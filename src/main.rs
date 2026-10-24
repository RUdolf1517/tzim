#![forbid(unsafe_code)]

use std::process::ExitCode;
use std::time::Instant;

use tzim::cli::{self, Command};
use tzim::error::{self, Result};
use tzim::formats;
use tzim::pipeline;
use tzim::pool;
use tzim::report::{self, Outcome, Status};
use tzim::walk;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

    match cli::parse(args.into_iter()) {
        Ok(Command::Help) => {
            print!("{}", cli::HELP);
            ExitCode::from(0)
        }
        Ok(Command::Version) => {
            println!("tzim {}", env!("CARGO_PKG_VERSION"));
            ExitCode::from(0)
        }
        Ok(Command::Formats) => {
            print_formats();
            ExitCode::from(0)
        }
        Ok(Command::Info) => {
            print_info();
            ExitCode::from(0)
        }
        Ok(Command::CheckDeps) => {
            print_check_deps();
            ExitCode::from(0)
        }
        Ok(Command::Optimize(args)) => match run_optimize(&args) {
            Ok(code) => ExitCode::from(code as u8),
            Err(e) => {
                eprintln!("tzim: {e}");
                ExitCode::from(3)
            }
        },
        Err(e) => {
            eprintln!("tzim: {e}");
            eprintln!("try `tzim --help`");
            ExitCode::from(2)
        }
    }
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
fn print_info() {
    println!("tzim {} — shrink the file, never the information", env!("CARGO_PKG_VERSION"));
    println!();
    println!("A lossless file optimizer. It makes a file smaller only when it can");
    println!("verify that nothing else changed, and leaves every file it cannot");
    println!("improve byte for byte alone: no metadata removal, no re-encoding by");
    println!("loss, no format conversion.");
    println!();
    println!("Formats in this build: {}", format_names());
    println!("JPEG is opt-in: name it with `--formats jpeg`.");
    println!();
    println!("Guarantees:");
    println!("  G1  byte-identical, wherever nothing was written");
    println!("  G3  format-equivalent, with a per-format contract checked before");
    println!("      the original is replaced");
    println!();
    println!("No external programs, no configuration file, no network, no telemetry.");
    println!();
    println!("Homepage  https://github.com/RUdolf1517/tzim");
    println!("Docs      https://github.com/RUdolf1517/tzim/blob/main/docs/cli.md");
    println!("License   MIT");
    println!("Author    RUdolf1517 <rudolfzinovev@protonmail.com>");
    println!();
    println!("Try `tzim photo.png`, `tzim --list-formats` or `tzim --check-deps`.");
}

/// `--check-deps`: the flag exists for parity with tools that need ffmpeg and
/// friends, and answers honestly. Tzim shells out to nothing, so the list is
/// empty and the exit code is 0 — a script can call it unconditionally.
fn print_check_deps() {
    println!("tzim needs no external programs at run time.");
    println!();
    println!("  binary     static, no C library, no shell-outs, no network");
    println!("  formats    {}", format_names());
    println!("  optional   --zopfli is compiled in, not an external encoder");
    println!();
    println!("Nothing to install. `tzim --list-formats` lists the handlers in this build.");
}

fn print_formats() {
    let rows = formats::supported();
    if rows.is_empty() {
        println!("No format handlers are compiled into this build.");
        println!("Rebuild with `--features png,gzip`.");
        return;
    }
    println!("{:<6} {:<46} PRESERVED", "FORMAT", "TRANSFORMATION");
    for (name, transformation, preserved) in rows {
        println!("{name:<6} {transformation:<46} {preserved}");
    }
    println!();
    println!("Tzim removes no metadata and never converts a file to another format.");
    println!("A result that is not strictly smaller is discarded and the original kept.");
}

fn run_optimize(args: &cli::Args) -> Result<i32> {
    let started = Instant::now();
    let jobs = if args.jobs == 0 {
        cli::default_jobs(args)
    } else {
        args.jobs
    };

    let scan = walk::scan(args);
    let config = pipeline::Config::from_args(args);
    let config = &config;

    let mut outcomes = pool::run(&scan.files, jobs, args.fail_fast, |path| {
        pipeline::process(path, config)
    });

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
    if args.json {
        report::print_json(&outcomes, &summary, args.dry_run);
    } else {
        report::print_human(&outcomes, &summary, args.quiet, args.dry_run);
    }

    let mut code = report::exit_code(&outcomes);
    if !scan.errors.is_empty() && code == 0 {
        code = 1;
    }
    Ok(code)
}

/// Scan errors are reported verbatim; they are re-created rather than cloned so
/// that `Error` does not need a `Clone` implementation.
fn clone_error(err: &error::Error) -> error::Error {
    match err {
        error::Error::Io(e) => error::Error::Io(std::io::Error::new(e.kind(), e.to_string())),
        error::Error::Parse(m) => error::Error::parse(m.clone()),
        error::Error::Limit(m) => error::Error::limit(m.clone()),
        error::Error::Refused(m) => error::Error::refused(m.clone()),
        error::Error::Verify(m) => error::Error::verify(m.clone()),
        error::Error::Usage(m) => error::Error::usage(m.clone()),
        error::Error::Internal(m) => error::Error::internal(m.clone()),
    }
}
