use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::detect::Format;
use crate::error::Error;

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

pub fn print_human(outcomes: &[Outcome], summary: &Summary, quiet: bool, dry_run: bool) {
    if !quiet {
        let width = outcomes
            .iter()
            .map(|o| display_width(&o.path.to_string_lossy()))
            .max()
            .unwrap_or(0)
            .clamp(1, 48);

        for o in outcomes {
            let path = o.path.to_string_lossy();
            let padded = format!("{path:<width$}");
            let format = o
                .format
                .map(|f| f.name())
                .unwrap_or("-")
                .to_string();
            let detail = match o.status {
                Status::Optimized | Status::WouldOptimize => {
                    let pct = if o.before == 0 {
                        0.0
                    } else {
                        100.0 * (o.before as f64 - o.after as f64) / o.before as f64
                    };
                    let suffix = if o.status == Status::WouldOptimize {
                        "  (dry run)"
                    } else {
                        ""
                    };
                    format!("{} -> {}  {:+.1}%{suffix}", size(o.before), size(o.after), -pct)
                }
                _ => match (&o.note, &o.error) {
                    (_, Some(e)) if o.status == Status::Failed => format!("failed: {e}"),
                    (Some(note), _) => format!("{}: {}", o.status.human(), note),
                    _ => o.status.human().to_string(),
                },
            };
            println!("{padded}  {format:<5}  {detail}");
        }
    }

    let action = if dry_run { "would optimize" } else { "optimized" };
    println!(
        "Summary: {} files, {} {action}, {} saved, {} already optimal, {} unsupported, {} skipped, {} failed, {:.2} s",
        summary.files,
        summary.optimized + summary.would_optimize,
        size(summary.before.saturating_sub(summary.after)),
        summary.already_optimal,
        summary.unsupported,
        summary.skipped,
        summary.failed,
        summary.elapsed.as_secs_f64()
    );
}

pub fn print_json(outcomes: &[Outcome], summary: &Summary, dry_run: bool) {
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
        line.push_str(&format!(",\"duration_ms\":{}", o.duration.as_millis()));
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
        println!("{line}");
    }

    println!(
        "{{\"type\":\"summary\",\"schema\":1,\"files\":{},\"optimized\":{},\"would_optimize\":{},\"already_optimal\":{},\"unsupported\":{},\"skipped\":{},\"failed\":{},\"before\":{},\"after\":{},\"saved\":{},\"duration_ms\":{},\"dry_run\":{}}}",
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
        dry_run
    );
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

/// Human-readable binary size with one decimal.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
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
        assert_eq!(size(1536), "1.50 KiB");
    }

    #[test]
    fn json_escaping() {
        let mut s = String::new();
        push_json_string(&mut s, "a\"b\\c\nd\u{1}");
        assert_eq!(s, "\"a\\\"b\\\\c\\nd\\u0001\"");
    }

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
