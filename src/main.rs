#![forbid(unsafe_code)]

use std::io::{self, BufWriter, IsTerminal, Write};
use std::process::ExitCode;
use std::time::Instant;

use tzim::cli::{self, Command};
use tzim::error;
use tzim::formats;
use tzim::pipeline;
use tzim::pool;
use tzim::progress::Progress;
use tzim::report::{self, Human, Outcome, Status};
use tzim::style::{self, Choice, Pen, Style};
use tzim::walk;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

    match cli::parse(args.into_iter()) {
        Ok(Command::Help) => emit(|out| write!(out, "{}", cli::HELP)),
        Ok(Command::Version) => emit(|out| writeln!(out, "tzim {}", env!("CARGO_PKG_VERSION"))),
        Ok(Command::Formats) => emit(print_formats),
        Ok(Command::Info) => emit(print_info),
        Ok(Command::CheckDeps) => emit(print_check_deps),
        Ok(Command::Optimize(args)) => ExitCode::from(run_optimize(&args) as u8),
        Err(e) => {
            let style = stderr_style(Choice::Auto);
            complain(&style, &e.to_string());
            if e.is_usage() {
                eprintln!("usage: tzim [OPTIONS] <PATH>...");
                eprintln!(
                    "{}",
                    style.paint(Pen::Dim, "try `tzim --help` for the full list")
                );
            }
            ExitCode::from(2)
        }
    }
}

/// Writes to stdout with one buffer, and answers a closed pipe the way a pipe
/// expects: silence and success.
fn emit(body: fn(&mut dyn Write) -> io::Result<()>) -> ExitCode {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let written = body(&mut out).and_then(|()| out.flush());
    finish(written)
}

fn finish(written: io::Result<()>) -> ExitCode {
    match written {
        Ok(()) => ExitCode::SUCCESS,
        // For the informational commands a closed pipe leaves nothing to do and
        // no result to report, so the exit code does not change.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tzim: cannot write to stdout: {e}");
            ExitCode::from(3)
        }
    }
}

/// Diagnostics go to stderr, coloured by the same rules as the report but
/// probed on stderr itself: a redirected stdout does not make a terminal's
/// stderr monochrome.
fn stderr_style(choice: Choice) -> Style {
    let probe = style::Probe::from_env(io::stderr().is_terminal());
    Style::new(style::enabled(choice, &probe))
}

fn complain(style: &Style, message: &str) {
    let mut line = String::with_capacity(message.len() + 16);
    style.paint_into(&mut line, Pen::Red, "tzim:");
    line.push(' ');
    line.push_str(message);
    eprintln!("{line}");
}

/// The format names this build can handle, in the order `--list-formats` prints
/// them, so the short line and the long table never disagree.
fn format_names() -> String {
    let names: Vec<&str> = formats::supported().iter().map(|row| row.0).collect();
    if names.is_empty() {
        "none compiled in".to_string()
    } else {
        names.join(", ")
    }
}

/// `-i`, `--info`: what this is, what it promises, and where it lives. No
/// environment probing, no colours, no pager — the same text on every platform.
fn print_info(out: &mut dyn Write) -> io::Result<()> {
    writeln!(
        out,
        "tzim {} — shrink the file, never the information",
        env!("CARGO_PKG_VERSION")
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "A lossless file optimizer. It makes a file smaller only when it can"
    )?;
    writeln!(
        out,
        "verify that nothing else changed, and leaves every file it cannot"
    )?;
    writeln!(out, "improve alone, byte for byte.")?;
    writeln!(out)?;
    writeln!(out, "Formats in this build: {}", format_names())?;
    // Both of these depend on how the binary was compiled, so both are read
    // from the build rather than asserted (`--features png` is a supported
    // build, and it has no JPEG and no Zopfli).
    writeln!(
        out,
        "{}",
        if cfg!(feature = "jpeg") {
            "JPEG is opt-in: name it with `--formats jpeg`."
        } else {
            "JPEG is not compiled into this build."
        }
    )?;
    // Which verification a JPEG candidate gets is a property of the build, and
    // this is the only place a user can ask (docs/guarantees.md §2.6, #66).
    if cfg!(feature = "jpeg") {
        writeln!(
            out,
            "{}",
            if cfg!(feature = "jpeg-v1") {
                "JPEG verification: an independent decoder (V1) plus the codec in this build (V3)."
            } else {
                "JPEG verification: the codec in this build only (V3); build with --features \
                 jpeg-v1 for an independent check."
            }
        )?;
    }
    writeln!(out)?;
    writeln!(out, "Guarantees:")?;
    writeln!(out, "  G1  byte-identical, wherever nothing was written")?;
    writeln!(
        out,
        "  G3  format-equivalent, with a per-format contract checked before"
    )?;
    writeln!(out, "      the original is replaced")?;
    writeln!(
        out,
        "  G4  metadata removed on request, and only with --strip-metadata"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "No external programs, no configuration file, no network, no telemetry."
    )?;
    writeln!(out)?;
    writeln!(out, "Homepage  https://github.com/RUdolf1517/tzim")?;
    writeln!(
        out,
        "Docs      https://github.com/RUdolf1517/tzim/blob/main/docs/cli.md"
    )?;
    writeln!(out, "License   MIT")?;
    writeln!(out, "Author    RUdolf1517 <rudolfzinovev@protonmail.com>")?;
    writeln!(out)?;
    writeln!(
        out,
        "Try `tzim photo.png`, `tzim --list-formats` or `tzim --check-deps`."
    )
}

/// `--check-deps`: the flag exists for parity with tools that need ffmpeg and
/// friends, and answers honestly. Tzim shells out to nothing, so the list is
/// empty and the exit code is 0 — a script can call it unconditionally.
fn print_check_deps(out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "tzim needs no external programs at run time.")?;
    writeln!(out)?;
    writeln!(
        out,
        "  binary     static, no C library, no shell-outs, no network"
    )?;
    writeln!(out, "  formats    {}", format_names())?;
    writeln!(
        out,
        "{}",
        if cfg!(feature = "zopfli") {
            "  optional   --zopfli is compiled into this build, not an external encoder"
        } else {
            "  optional   --zopfli is not compiled into this build"
        }
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "Nothing to install. `tzim --list-formats` lists the handlers in this build."
    )
}

fn print_formats(out: &mut dyn Write) -> io::Result<()> {
    let rows = formats::supported();
    if rows.is_empty() {
        writeln!(out, "No format handlers are compiled into this build.")?;
        writeln!(out, "Rebuild with `--features png,gzip`.")?;
        return Ok(());
    }
    writeln!(out, "{:<6} {:<46} PRESERVED", "FORMAT", "TRANSFORMATION")?;
    for (name, transformation, preserved) in rows {
        writeln!(out, "{name:<6} {transformation:<46} {preserved}")?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "Tzim removes no metadata unless --strip-metadata is given, and never"
    )?;
    writeln!(out, "converts a file to another format.")?;
    writeln!(
        out,
        "A result that is not smaller is discarded and the original kept; with"
    )?;
    writeln!(
        out,
        "--strip-metadata an equal-size result is accepted, because zeroing a"
    )?;
    writeln!(out, "timestamp removes metadata without removing bytes.")
}

fn run_optimize(args: &cli::Args) -> i32 {
    let started = Instant::now();
    let jobs = if args.jobs == 0 {
        cli::default_jobs(args)
    } else {
        args.jobs
    };

    let scan = walk::scan(args);
    let config = pipeline::Config::from_args(args);
    let config = &config;

    // The animation is decided before any work starts and lives only as long as
    // it: `drop(progress)` erases the line before the first result is printed.
    // Colour is the same decision the report uses, taken from stderr's own probe.
    let progress = Progress::new(
        scan.files.len(),
        progress_wanted(args, scan.files.len()),
        stderr_style(args.color),
    );
    let mut outcomes = pool::run(
        &scan.files,
        jobs,
        args.fail_fast,
        |path| pipeline::process(path, config),
        |_| progress.advance(),
    );
    drop(progress);

    // Paths that never reached the pipeline still need a report line: silently
    // dropping an argument would be exactly the kind of surprise to avoid.
    for (path, reason) in &scan.skipped {
        let mut outcome = Outcome::new(path.clone(), Status::Skipped, 0);
        outcome.note = Some(reason.clone());
        outcomes.push(outcome);
    }
    for (path, err) in &scan.errors {
        let mut outcome = Outcome::new(path.clone(), Status::Failed, 0);
        outcome.error = Some(clone_error(err));
        outcomes.push(outcome);
    }

    let summary = report::summarize(&outcomes, started.elapsed());
    let mut code = report::exit_code(&outcomes);
    if !scan.errors.is_empty() && code == 0 {
        code = 1;
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let written = if args.json {
        report::print_json(
            &mut out,
            &outcomes,
            &summary,
            args.dry_run,
            args.strip_metadata,
        )
    } else {
        let style = stderr_style(args.color);
        let opts = Human {
            quiet: args.quiet,
            dry_run: args.dry_run,
            strip_metadata: args.strip_metadata,
            style,
            summary: &summary,
        };
        report::print_human(&mut out, &outcomes, &opts)
    };
    match written.and_then(|()| out.flush()) {
        Ok(()) => code,
        // Nobody is listening: say nothing, and let the exit code describe the
        // work rather than the pipe. `tzim . -r | head -1` is not a failure.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => code,
        Err(e) => {
            eprintln!("tzim: cannot write to stdout: {e}");
            3
        }
    }
}

/// Whether to animate. A line of `\r` frames belongs on a terminal, never in a
/// log file, a CI transcript or a pipe someone is parsing.
fn progress_wanted(args: &cli::Args, files: usize) -> bool {
    files > 0 && !args.no_progress && !args.quiet && !args.json && io::stderr().is_terminal()
}

/// Scan errors are reported verbatim; they are re-created rather than cloned so
/// that `Error` does not need a `Clone` implementation.
fn clone_error(err: &error::Error) -> error::Error {
    match err {
        error::Error::Io(e) => error::Error::Io(std::io::Error::new(e.kind(), e.to_string())),
        error::Error::Parse(m) => error::Error::parse(m.clone()),
        error::Error::Limit(m) => error::Error::limit(m.clone()),
        error::Error::Refused(m) => error::Error::refused(m.clone()),
        error::Error::Unsupported(m) => error::Error::unsupported(m.clone()),
        error::Error::Verify(m) => error::Error::verify(m.clone()),
        error::Error::Usage(m) => error::Error::usage(m.clone()),
        error::Error::Internal(m) => error::Error::internal(m.clone()),
    }
}
