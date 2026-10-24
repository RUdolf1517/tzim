use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Parsed command line.
#[derive(Debug)]
pub enum Command {
    Optimize(Args),
    Formats,
    Help,
    Version,
}

#[derive(Debug)]
pub struct Args {
    pub paths: Vec<PathBuf>,
    pub recursive: bool,
    pub dry_run: bool,
    pub jobs: usize,
    pub json: bool,
    pub quiet: bool,
    pub fail_fast: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub min_size: u64,
    pub max_size: u64,
    pub max_pixels: u64,
    pub max_uncompressed: u64,
    pub max_ratio: u64,
    pub nested: bool,
    pub max_depth: u32,
    pub zopfli: bool,
    pub follow_symlinks: bool,
    pub allow_hardlinks: bool,
    pub allow_root_ownership_change: bool,
    pub no_fsync: bool,
}

pub const DEFAULT_MAX_SIZE: u64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_PIXELS: u64 = 100_000_000;
pub const DEFAULT_MAX_UNCOMPRESSED: u64 = 8 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_RATIO: u64 = 1000;

impl Default for Args {
    fn default() -> Self {
        Args {
            paths: Vec::new(),
            recursive: false,
            dry_run: false,
            jobs: 0, // 0 means "decide from the CPU count"
            json: false,
            quiet: false,
            fail_fast: false,
            include: Vec::new(),
            exclude: Vec::new(),
            min_size: 0,
            max_size: DEFAULT_MAX_SIZE,
            max_pixels: DEFAULT_MAX_PIXELS,
            max_uncompressed: DEFAULT_MAX_UNCOMPRESSED,
            max_ratio: DEFAULT_MAX_RATIO,
            nested: false,
            max_depth: 3,
            zopfli: false,
            follow_symlinks: false,
            allow_hardlinks: false,
            allow_root_ownership_change: false,
            no_fsync: false,
        }
    }
}

pub const HELP: &str = "\
tzim - shrink the file, never the information

USAGE:
    tzim optimize [OPTIONS] <PATH>...
    tzim formats
    tzim --help
    tzim --version

PATHS:
    A regular file, a directory, or a mix of both. Directories are processed
    at the top level unless --recursive is given. Symbolic links are skipped
    unless --follow-symlinks is given. Because a directory is never necessary
    to process a single file, a bare file path is a complete input object.

OPTIONS:
    -r, --recursive            descend into directories
    -n, --dry-run              do all the work, write nothing, report what would change
    -j, --jobs <N>             maximum files processed at once (default: min(cores, 4))
        --json                 newline-delimited JSON on stdout
    -q, --quiet                suppress per-file output
        --fail-fast            stop after the first failed file
        --include <EXT,...>    only files with these extensions
        --exclude <EXT,...>    skip files with these extensions
        --min-size <BYTES>     skip smaller files (default: 0)
        --max-size <BYTES>     skip larger files (default: 2147483648)
        --max-pixels <N>       skip images with more pixels (default: 100000000)
        --max-uncompressed <BYTES>
                               refuse archives that expand beyond this (default: 8589934592)
        --max-ratio <N>        refuse archives with a ratio above N:1 (default: 1000)
        --nested               also optimize supported files inside archives
        --max-depth <N>        nesting depth limit (default: 3)
        --zopfli               use the slower, stronger encoder
        --follow-symlinks      resolve and follow symbolic links
        --allow-hardlinks      process files with more than one hard link
        --allow-root-ownership-change
                               when running as root, replace files owned by others
        --no-fsync             skip fsync (faster, less durable)

EXIT CODES:
    0  success (including files that were already optimal, skipped or unsupported)
    1  at least one file failed; the others were still processed
    2  invalid usage; nothing was processed
    3  fatal error; the run could not be completed
    4  verification failure; the affected file was not replaced

Tzim never removes metadata, never re-encodes lossily and never converts
formats. If the result is not strictly smaller, the original is left alone.
";

pub fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command> {
    let first = match args.next() {
        Some(a) => a,
        None => return Err(Error::usage("no command given; try `tzim --help`")),
    };

    match first.to_str() {
        Some("--help") | Some("-h") => return Ok(Command::Help),
        Some("--version") | Some("-V") => return Ok(Command::Version),
        Some("formats") => return Ok(Command::Formats),
        Some("optimize") => {}
        Some(other) if other.starts_with('-') => {
            return Err(Error::usage(format!("unknown option `{other}`")));
        }
        Some(other) => {
            return Err(Error::usage(format!(
                "unknown command `{other}`; expected `optimize`, `formats`, `--help` or `--version`"
            )));
        }
        None => return Err(Error::usage("command is not valid UTF-8")),
    }

    let mut out = Args::default();
    let mut only_paths = false;

    while let Some(arg) = args.next() {
        let text = match arg.to_str() {
            Some(t) => t.to_owned(),
            None => {
                // A non-UTF-8 argument is a path, never an option.
                out.paths.push(PathBuf::from(arg));
                continue;
            }
        };

        if only_paths || !text.starts_with('-') || text == "-" {
            out.paths.push(PathBuf::from(arg));
            continue;
        }
        if text == "--" {
            only_paths = true;
            continue;
        }

        // Split `--opt=value`.
        let (name, inline) = match text.split_once('=') {
            Some((n, v)) => (n.to_owned(), Some(v.to_owned())),
            None => (text.clone(), None),
        };

        let mut take_value = |inline: Option<String>| -> Result<String> {
            match inline {
                Some(v) => Ok(v),
                None => match args.next() {
                    Some(v) => Ok(v.to_string_lossy().into_owned()),
                    None => Err(Error::usage(format!("option `{name}` needs a value"))),
                },
            }
        };

        match name.as_str() {
            "-r" | "--recursive" => out.recursive = true,
            "-n" | "--dry-run" => out.dry_run = true,
            "-q" | "--quiet" => out.quiet = true,
            "--json" => out.json = true,
            "--fail-fast" => out.fail_fast = true,
            "--zopfli" => out.zopfli = true,
            "--follow-symlinks" => out.follow_symlinks = true,
            "--allow-hardlinks" => out.allow_hardlinks = true,
            "--allow-root-ownership-change" => out.allow_root_ownership_change = true,
            "--no-fsync" => out.no_fsync = true,
            "-j" | "--jobs" => {
                let v = take_value(inline)?;
                out.jobs = v
                    .parse::<usize>()
                    .map_err(|_| Error::usage(format!("`--jobs` expects a number, got `{v}`")))?;
                if out.jobs == 0 {
                    return Err(Error::usage("`--jobs` must be at least 1"));
                }
            }
            "--include" => {
                let v = take_value(inline)?;
                out.include.extend(split_list(&v));
            }
            "--exclude" => {
                let v = take_value(inline)?;
                out.exclude.extend(split_list(&v));
            }
            "--min-size" => out.min_size = parse_u64(&name, &take_value(inline)?)?,
            "--max-size" => out.max_size = parse_u64(&name, &take_value(inline)?)?,
            "--max-pixels" => out.max_pixels = parse_u64(&name, &take_value(inline)?)?,
            "--max-uncompressed" => out.max_uncompressed = parse_u64(&name, &take_value(inline)?)?,
            "--max-ratio" => out.max_ratio = parse_u64(&name, &take_value(inline)?)?,
            "--nested" => out.nested = true,
            "--max-depth" => {
                let v = take_value(inline)?;
                let depth = v
                    .parse::<u32>()
                    .map_err(|_| Error::usage(format!("`--max-depth` expects a number, got `{v}`")))?;
                if depth == 0 {
                    return Err(Error::usage("`--max-depth` must be at least 1"));
                }
                out.max_depth = depth;
            }
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            _ => return Err(Error::usage(format!("unknown option `{name}`"))),
        }
    }

    if out.paths.is_empty() {
        return Err(Error::usage("no paths given; try `tzim --help`"));
    }
    if out.min_size > 0 && out.max_size < out.min_size {
        return Err(Error::usage("`--max-size` is smaller than `--min-size`"));
    }

    Ok(Command::Optimize(out))
}

fn parse_u64(name: &str, value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|_| Error::usage(format!("`{name}` expects a number of bytes, got `{value}`")))
}

/// Accepts `png,gz` and `.png, .gz`; normalizes to lowercase without dots.
fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Default parallelism: bounded, and lower when the CPU-hungry encoder is on.
pub fn default_jobs(args: &Args) -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let cap = if args.zopfli { 2 } else { 4 };
    cpus.min(cap).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(items: &[&str]) -> Args {
        let args: Vec<OsString> = items.iter().map(OsString::from).collect();
        match parse(args.into_iter()).expect("parse should succeed") {
            Command::Optimize(a) => a,
            _ => panic!("expected optimize"),
        }
    }

    #[test]
    fn minimal_command() {
        let args = parse_ok(&["optimize", "a.png"]);
        assert_eq!(args.paths.len(), 1);
        assert!(!args.recursive);
        assert_eq!(args.max_size, DEFAULT_MAX_SIZE);
    }

    #[test]
    fn flags_and_values() {
        let args = parse_ok(&[
            "optimize",
            "-r",
            "--dry-run",
            "--jobs",
            "2",
            "--json",
            "--include",
            "png,.GZ",
            "dir",
        ]);
        assert!(args.recursive && args.dry_run && args.json);
        assert_eq!(args.jobs, 2);
        assert_eq!(args.include, vec!["png", "gz"]);
        assert_eq!(args.paths, vec![PathBuf::from("dir")]);
    }

    #[test]
    fn inline_value_and_double_dash() {
        let args = parse_ok(&["optimize", "--jobs=3", "--", "--weird-name.png"]);
        assert_eq!(args.jobs, 3);
        assert_eq!(args.paths, vec![PathBuf::from("--weird-name.png")]);
    }

    #[test]
    fn rejects_unknown_option() {
        let args: Vec<OsString> = ["optimize", "--nope", "x"].iter().map(OsString::from).collect();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn rejects_missing_paths() {
        let args: Vec<OsString> = ["optimize"].iter().map(OsString::from).collect();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn rejects_zero_jobs() {
        let args: Vec<OsString> = ["optimize", "--jobs", "0", "x"].iter().map(OsString::from).collect();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn help_and_version() {
        let args: Vec<OsString> = ["--help"].iter().map(OsString::from).collect();
        assert!(matches!(parse(args.into_iter()), Ok(Command::Help)));
        let args: Vec<OsString> = ["--version"].iter().map(OsString::from).collect();
        assert!(matches!(parse(args.into_iter()), Ok(Command::Version)));
    }
}
