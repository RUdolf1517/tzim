use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::detect::Format;
use crate::error::Error;
use crate::style::{Pen, Style};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Optimized,
    WouldOptimize,
    AlreadyOptimal,
    Unsupported,
    Skipped,
    Failed,
}

impl Status {
    pub fn id(self) -> &'static str {
        match self {
            Status::Optimized => "optimized",
            Status::WouldOptimize => "would_optimize",
            Status::AlreadyOptimal => "already_optimal",
            Status::Unsupported => "unsupported",
            Status::Skipped => "skipped",
            Status::Failed => "failed",
        }
    }

    fn human(self) -> &'static str {
        match self {
            Status::Optimized => "optimized",
            Status::WouldOptimize => "would optimize",
            Status::AlreadyOptimal => "already optimal",
            Status::Unsupported => "unsupported",
            Status::Skipped => "skipped",
            Status::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Guarantee {
    ByteIdentical,
    FormatEquivalent,
}

impl Guarantee {
    fn id(self) -> &'static str {
        match self {
            Guarantee::ByteIdentical => "byte_identical",
            Guarantee::FormatEquivalent => "format_equivalent",
        }
    }
}

/// The single record produced for every path, whatever happened to it.
pub struct Outcome {
    pub path: PathBuf,
    pub format: Option<Format>,
    pub status: Status,
    pub before: u64,
    pub after: u64,
    pub duration: Duration,
    pub note: Option<String>,
    pub error: Option<Error>,
}

impl Outcome {
    pub fn new(path: PathBuf, status: Status, before: u64) -> Outcome {
        Outcome {
            path,
            format: None,
            status,
            before,
            after: before,
            duration: Duration::from_secs(0),
            note: None,
            error: None,
        }
    }

    pub fn guarantee(&self) -> Guarantee {
        match self.status {
            Status::Optimized | Status::WouldOptimize => Guarantee::FormatEquivalent,
            _ => Guarantee::ByteIdentical,
        }
    }

    pub fn saved(&self) -> u64 {
        self.before.saturating_sub(self.after)
    }

    fn failed_by_verify(&self) -> bool {
        self.error.as_ref().map(|e| e.is_verify()).unwrap_or(false)
    }
}

pub struct Summary {
    pub files: usize,
    pub optimized: usize,
    pub would_optimize: usize,
    pub already_optimal: usize,
    pub unsupported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub before: u64,
    pub after: u64,
    pub elapsed: Duration,
}

pub fn summarize(outcomes: &[Outcome], elapsed: Duration) -> Summary {
    let mut s = Summary {
        files: outcomes.len(),
        optimized: 0,
        would_optimize: 0,
        already_optimal: 0,
        unsupported: 0,
        skipped: 0,
        failed: 0,
        before: 0,
        after: 0,
        elapsed,
    };
    for o in outcomes {
        match o.status {
            Status::Optimized => s.optimized += 1,
            Status::WouldOptimize => s.would_optimize += 1,
            Status::AlreadyOptimal => s.already_optimal += 1,
            Status::Unsupported => s.unsupported += 1,
            Status::Skipped => s.skipped += 1,
            Status::Failed => s.failed += 1,
        }
        s.before += o.before;
        s.after += o.after;
    }
    s
}

/// Exit code for the run. Verification failures dominate, then I/O failures.
pub fn exit_code(outcomes: &[Outcome]) -> i32 {
    if outcomes.iter().any(|o| o.failed_by_verify()) {
        return 4;
    }
    if outcomes.iter().any(|o| o.status == Status::Failed) {
        return 1;
    }
    0
}

/// Everything the human report needs from the command line.
pub struct Human<'a> {
    pub quiet: bool,
    pub dry_run: bool,
    pub strip_metadata: bool,
    pub style: Style,
    pub summary: &'a Summary,
}

/// Writes the per-file lines and then the summary.
///
/// The sink is a parameter rather than `stdout!()` so that a closed pipe comes
/// back as an `io::Error` the caller can recognise, instead of a panic inside
/// `println!` — which, with `panic = "abort"`, kills the process with a
/// backtrace-shaped message the user cannot act on.
pub fn print_human<W: Write + ?Sized>(
    out: &mut W,
    outcomes: &[Outcome],
    opts: &Human<'_>,
) -> io::Result<()> {
    let style = opts.style;
    if !opts.quiet {
        let width = outcomes
            .iter()
            .map(|o| display_width(&o.path.to_string_lossy()))
            .max()
            .unwrap_or(0)
            .clamp(1, 48);

        for o in outcomes {
            let path = o.path.to_string_lossy();
            let mut line = String::with_capacity(path.len() + 72);
            line.push_str(&format!("{path:<width$}"));
            line.push_str("  ");
            // The format column is context, not a result: dimming it keeps the
            // eye on the status and the number.
            let name = o.format.map(|f| f.name()).unwrap_or("-");
            style.paint_into(&mut line, Pen::Dim, &format!("{name:<5}"));
            line.push_str("  ");
            push_detail(&mut line, o, opts.strip_metadata, style);
            writeln!(out, "{line}")?;
        }
    }
    push_summary(out, opts.summary, opts.dry_run, style)
}

/// One line per file, with colour only on the outcome.
fn push_detail(into: &mut String, o: &Outcome, strip_metadata: bool, style: Style) {
    if matches!(o.status, Status::Optimized | Status::WouldOptimize) {
        // A gain is printed as a negative number, exactly as the old float
        // formatting did, including "-0.0" when nothing moved.
        let (sign, tenths) = match o.before {
            0 => ('-', 0),
            _ if o.after <= o.before => (
                '-',
                round_half_even((o.before - o.after) as u128 * 1000, o.before as u128),
            ),
            _ => (
                '+',
                round_half_even((o.after - o.before) as u128 * 1000, o.before as u128),
            ),
        };
        into.push_str(&format!("{} -> {}  ", size(o.before), size(o.after)));
        let percent = format!("{sign}{}.{}%", tenths / 10, tenths % 10);
        let pen = if o.after < o.before {
            Pen::Green
        } else {
            Pen::Dim
        };
        style.paint_into(into, pen, &percent);
        if o.status == Status::WouldOptimize {
            style.paint_into(into, Pen::Dim, "  (dry run)");
        }
        if strip_metadata {
            // The only status where the output is not a faithful copy of the
            // input's metadata, so it is named on every line.
            style.paint_into(into, Pen::Dim, "  (metadata stripped)");
        }
        return;
    }

    let pen = match o.status {
        Status::Failed => Pen::Red,
        Status::Skipped => Pen::Yellow,
        _ => Pen::Dim,
    };
    style.paint_into(into, pen, o.status.human());
    if let (Status::Failed, Some(e)) = (&o.status, &o.error) {
        into.push_str(&format!(": {e}"));
        return;
    }
    if let Some(note) = &o.note {
        into.push_str(&format!(": {note}"));
    }
}

/// The summary is the last thing on screen, so it carries the numbers that
/// decide whether the run was worth it: the count that moved and the bytes.
fn push_summary<W: Write + ?Sized>(
    out: &mut W,
    summary: &Summary,
    dry_run: bool,
    style: Style,
) -> io::Result<()> {
    let action = if dry_run {
        "would optimize"
    } else {
        "optimized"
    };
    let saved = summary.before.saturating_sub(summary.after);
    let hundredths = (summary.elapsed.as_micros() + 5_000) / 10_000;

    let mut line = String::with_capacity(160);
    style.paint_into(&mut line, Pen::Bold, "Summary:");
    line.push_str(&format!(
        " {} files, {} {action}, ",
        summary.files,
        summary.optimized + summary.would_optimize
    ));
    let saved_text = format!("{} saved", size(saved));
    if saved > 0 {
        style.paint_into(&mut line, Pen::Green, &saved_text);
    } else {
        line.push_str(&saved_text);
    }
    line.push_str(&format!(
        ", {} already optimal, {} unsupported, {} skipped, ",
        summary.already_optimal, summary.unsupported, summary.skipped
    ));
    let failed_text = format!("{} failed", summary.failed);
    if summary.failed > 0 {
        style.paint_into(&mut line, Pen::Red, &failed_text);
    } else {
        line.push_str(&failed_text);
    }
    line.push_str(&format!(", {}.{:02} s", hundredths / 100, hundredths % 100));
    writeln!(out, "{line}")
}

/// Newline-delimited JSON. Never coloured, whatever `--color` says: the schema
/// is for programs, and an escape sequence inside a JSON string is a bug in the
/// producer, not a feature of the consumer.
pub fn print_json<W: Write + ?Sized>(
    out: &mut W,
    outcomes: &[Outcome],
    summary: &Summary,
    dry_run: bool,
    strip_metadata: bool,
) -> io::Result<()> {
    for o in outcomes {
        let mut line = String::with_capacity(256);
        line.push_str("{\"type\":\"file\",\"path\":");
        push_json_string(&mut line, &o.path.to_string_lossy());
        push_raw_path(&mut line, &o.path);
        line.push_str(",\"format\":");
        match o.format {
            Some(f) => push_json_string(&mut line, f.id()),
            None => line.push_str("null"),
        }
        line.push_str(",\"status\":");
        push_json_string(&mut line, o.status.id());
        line.push_str(&format!(",\"before\":{},\"after\":{}", o.before, o.after));
        line.push_str(&format!(",\"saved\":{}", o.saved()));
        line.push_str(",\"guarantee\":");
        push_json_string(&mut line, o.guarantee().id());
        if strip_metadata && matches!(o.status, Status::Optimized | Status::WouldOptimize) {
            line.push_str(",\"metadata_stripped\":true");
        }
        line.push_str(&format!(",\"duration_ms\":{}", o.duration.as_millis()));
        line.push_str(",\"note\":");
        match &o.note {
            Some(note) => push_json_string(&mut line, note),
            None => line.push_str("null"),
        }
        line.push_str(",\"error\":");
        match (&o.status, &o.error) {
            (Status::Failed, Some(e)) => {
                line.push_str("{\"kind\":");
                push_json_string(&mut line, e.kind());
                line.push_str(",\"message\":");
                push_json_string(&mut line, &e.to_string());
                line.push('}');
            }
            _ => line.push_str("null"),
        }
        line.push('}');
        writeln!(out, "{line}")?;
    }

    writeln!(
        out,
        "{{\"type\":\"summary\",\"schema\":1,\"files\":{},\"optimized\":{},\"would_optimize\":{},\"already_optimal\":{},\"unsupported\":{},\"skipped\":{},\"failed\":{},\"before\":{},\"after\":{},\"saved\":{},\"duration_ms\":{},\"dry_run\":{},\"strip_metadata\":{}}}",
        summary.files,
        summary.optimized,
        summary.would_optimize,
        summary.already_optimal,
        summary.unsupported,
        summary.skipped,
        summary.failed,
        summary.before,
        summary.after,
        summary.before.saturating_sub(summary.after),
        summary.elapsed.as_millis(),
        dry_run,
        strip_metadata
    )
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

/// Human-readable binary size with two decimals.
///
/// The arithmetic is integer on purpose. `{:.2}` on an `f64` pulls the float
/// formatter into the binary — it is larger than all of the rest of the
/// reporting code — and the numbers here are exact in integers: the divisor is
/// a power of two, so the quotient has no representation error and
/// half-to-even rounding reproduces `{:.2}` digit for digit.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 1;
    let mut scale = 1024u64;
    while bytes / scale >= 1024 && unit + 1 < UNITS.len() {
        scale *= 1024;
        unit += 1;
    }
    let hundredths = round_half_even(bytes as u128 * 100, scale as u128);
    format!(
        "{}.{:02} {}",
        hundredths / 100,
        hundredths % 100,
        UNITS[unit]
    )
}

/// `value / divisor`, rounded to nearest with ties to even.
fn round_half_even(value: u128, divisor: u128) -> u128 {
    let quotient = value / divisor;
    let twice = (value % divisor) * 2;
    if twice > divisor || (twice == divisor && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    }
}

pub fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// On Unix, a path may not be valid UTF-8. The human field is lossy; the raw
/// bytes are also emitted, base64-encoded, so nothing is lost for scripts.
fn push_raw_path(out: &mut String, path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_os_str().as_bytes();
        if std::str::from_utf8(bytes).is_err() {
            out.push_str(",\"path_raw_b64\":\"");
            out.push_str(&base64(bytes));
            out.push('"');
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (out, path);
    }
}

#[cfg(unix)]
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1024), "1.00 KiB");
        assert_eq!(size(1536), "1.50 KiB");
        assert_eq!(size(1_048_576), "1.00 MiB");
        assert_eq!(size(1_048_575), "1024.00 KiB");
        assert_eq!(size(1024 * 1024 * 1024 + 512 * 1024), "1.00 GiB");
    }

    #[test]
    fn json_escaping() {
        let mut s = String::new();
        push_json_string(&mut s, "a\"b\\c\nd\u{1}");
        assert_eq!(s, "\"a\\\"b\\\\c\\nd\\u0001\"");
    }

    #[cfg(unix)]
    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xff, 0xfe]), "//4=");
    }

    #[test]
    fn exit_codes() {
        let mut ok = Outcome::new(PathBuf::from("a"), Status::Optimized, 10);
        ok.after = 5;
        assert_eq!(exit_code(&[ok]), 0);

        let failed = Outcome::new(PathBuf::from("b"), Status::Failed, 10);
        assert_eq!(exit_code(&[failed]), 1);

        let mut verify = Outcome::new(PathBuf::from("c"), Status::Failed, 10);
        verify.error = Some(Error::verify("contract violated"));
        assert_eq!(exit_code(&[verify]), 4);
    }
}
