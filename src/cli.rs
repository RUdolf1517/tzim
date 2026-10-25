use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::formats::Allow;
use crate::style::Choice as ColorChoice;

/// Parsed command line.
#[derive(Debug)]
pub enum Command {
    Optimize(Args),
    Formats,
    Info,
    CheckDeps,
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
    /// The most any one job may be expected to need, in bytes (#51).
    pub memory_budget: u64,
    pub strip_metadata: bool,
    pub nested: bool,
    pub max_depth: u32,
    pub allow: Allow,
    pub zopfli: bool,
    pub follow_symlinks: bool,
    pub allow_hardlinks: bool,
    pub allow_root_ownership_change: bool,
    pub no_fsync: bool,
    pub color: ColorChoice,
    pub no_progress: bool,
}

pub const DEFAULT_MAX_SIZE: u64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_PIXELS: u64 = 100_000_000;
pub const DEFAULT_MAX_UNCOMPRESSED: u64 = 8 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_RATIO: u64 = 1000;
/// Zero asks for a budget derived from the other limits, which is what the
/// defaults do: see `pipeline::derived_memory_budget`. Lower it to what the
/// machine actually has — that is the point of the flag (docs/safety.md §7.1).
pub const DEFAULT_MEMORY_BUDGET: u64 = 0;

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
            memory_budget: DEFAULT_MEMORY_BUDGET,
            strip_metadata: false,
            nested: false,
            max_depth: 3,
            allow: Allow::default(),
            zopfli: false,
            follow_symlinks: false,
            allow_hardlinks: false,
            allow_root_ownership_change: false,
            no_fsync: false,
            color: ColorChoice::Auto,
            no_progress: false,
        }
    }
}

pub const HELP: &str = "\
tzim - shrink the file, never the information

USAGE:
    tzim [OPTIONS] <PATH>...
    tzim --list-formats
    tzim --info
    tzim --check-deps
    tzim --help
    tzim --version

PATHS:
    A regular file, a directory, or a mix of both. A bare file path is a
    complete input object, so a single file never needs a wrapper directory,
    and files, directories and duplicates may be mixed in one run.
    Directories are processed at the top level unless --recursive is given.
    Symbolic links are skipped unless --follow-symlinks is given.

OPTIONS:
    -r, --recursive            descend into directories
    -n, --dry-run              do all the work, write nothing, report what would change
    -j, --jobs <N>             maximum files processed at once (default: min(cores, 4))
        --json                 newline-delimited JSON on stdout
    -q, --quiet                suppress per-file output
        --color <WHEN>         colour the report: auto (default), always, never
        --no-color             same as `--color=never`
        --no-progress          no progress line on stderr
        --fail-fast            stop after the first failed file
        --include <EXT,...>    only files with these extensions
        --exclude <EXT,...>    skip files with these extensions
        --min-size <BYTES>     skip smaller files (default: 0)
        --max-size <BYTES>     skip larger files (default: 2147483648)
        --max-pixels <N>       skip images with more pixels (default: 100000000)
        --max-uncompressed <BYTES>
                               refuse archives that expand beyond this (default: 8589934592)
        --max-ratio <N>        refuse archives with a ratio above N:1 (default: 1000)
        --memory-budget <BYTES>
                               skip a file whose worst case would exceed this
                               many bytes; default derived from the limits above
        --nested               also optimize supported files inside archives
        --max-depth <N>        nesting depth limit (default: 3)
        --formats <LIST>       only process these formats: png, gzip, zip, ico,
                               jpeg, tiff, zlib; JPEG is off unless named here
        --zopfli               use the slower, stronger encoder
        --follow-symlinks      resolve and follow symbolic links
        --allow-hardlinks      process files with more than one hard link
        --allow-root-ownership-change
                               when running as root, replace files owned by others
        --no-fsync             skip fsync (faster, less durable)

METADATA:
        --strip-metadata       remove the documented metadata list: PNG text and
                               timestamps, gzip name and comment, ZIP comments,
                               JPEG Exif and comments. Not lossless, never on by
                               default, never implied by another flag.

INFORMATION:
    -i, --info                 what this program is: version, promise, links
        --list-formats         the handlers compiled into this build
        --check-deps           external programs needed at run time (none)
    -h, --help                 this text
    -V, --version              the version, one line

EXIT CODES:
    0  success (including files that were already optimal, skipped or unsupported)
    1  at least one file failed; the others were still processed
    2  invalid usage; nothing was processed
    3  fatal error; the run could not be completed
    4  verification failure; the affected file was not replaced

COLOUR:
    On only when the stream is a terminal. `NO_COLOR` (non-empty), `CLICOLOR=0`
    and `TERM=dumb` turn it off; `CLICOLOR_FORCE` turns it on. --json is never
    coloured. The progress line follows the same rules and needs a terminal on
    stderr.

COMMAND-LINE CONVENTIONS:
    Options may appear before, between or after paths; `-rq` and `-j4` are
    accepted; `--` ends option parsing. Unknown options exit 2 and print usage.

Tzim never re-encodes lossily and never converts formats. Metadata is kept
unless --strip-metadata asks for a fixed, documented list to be removed. If the
result is not strictly smaller, the original is left alone; with
--strip-metadata an equal-size rewrite is accepted, because zeroing a timestamp
removes information without removing bytes.
";

pub fn parse(args: impl Iterator<Item = OsString>) -> Result<Command> {
    let mut argv: Vec<OsString> = args.collect();
    if argv.is_empty() {
        return Err(Error::usage("no paths given; try `tzim --help`"));
    }

    // `tzim optimize file` was the shape before 0.5. Paths come first now, and
    // the old verb is still accepted so that scripts and documentation written
    // against 0.1-0.4 keep working. `formats` likewise survives as an alias.
    if argv[0].to_str() == Some("optimize") {
        argv.remove(0);
    } else if argv[0].to_str() == Some("formats") {
        return Ok(Command::Formats);
    }
    if argv.is_empty() {
        return Err(Error::usage("no paths given; try `tzim --help`"));
    }

    let mut args = argv.into_iter();
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
            "--strip-metadata" => out.strip_metadata = true,
            "-q" | "--quiet" => out.quiet = true,
            "--json" => out.json = true,
            "--fail-fast" => out.fail_fast = true,
            "--zopfli" => out.zopfli = true,
            "--follow-symlinks" => out.follow_symlinks = true,
            "--allow-hardlinks" => out.allow_hardlinks = true,
            "--allow-root-ownership-change" => out.allow_root_ownership_change = true,
            "--no-fsync" => out.no_fsync = true,
            "-j" | "--jobs" => out.jobs = parse_jobs(&take_value(inline)?)?,
            "--color" => {
                let v = take_value(inline)?;
                out.color = ColorChoice::parse(&v).ok_or_else(|| {
                    Error::usage(format!(
                        "`--color` expects auto, always or never, got `{v}`"
                    ))
                })?;
            }
            "--no-color" => out.color = ColorChoice::Never,
            "--no-progress" => out.no_progress = true,
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
            "--memory-budget" => out.memory_budget = parse_u64(&name, &take_value(inline)?)?,
            "--formats" => {
                let v = take_value(inline)?;
                let names = split_list(&v);
                if names.is_empty() {
                    return Err(Error::usage("`--formats` needs at least one format name"));
                }
                out.allow = Allow::named(&names)?;
            }
            "--nested" => out.nested = true,
            "--max-depth" => {
                let v = take_value(inline)?;
                let depth = v.parse::<u32>().map_err(|_| {
                    Error::usage(format!("`--max-depth` expects a number, got `{v}`"))
                })?;
                if depth == 0 {
                    return Err(Error::usage("`--max-depth` must be at least 1"));
                }
                out.max_depth = depth;
            }
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "-i" | "--info" => return Ok(Command::Info),
            "--list-formats" => return Ok(Command::Formats),
            "--check-deps" => return Ok(Command::CheckDeps),
            _ => {
                // Clustered short flags (`-rq`) and attached values (`-j4`), the
                // way getopt has always allowed them. A long option never
                // reaches this arm: `--color=never` was already split above.
                if name.len() > 2 && !name.starts_with("--") {
                    let letters: Vec<char> = name[1..].chars().collect();
                    let mut index = 0;
                    while index < letters.len() {
                        match letters[index] {
                            'r' => out.recursive = true,
                            'n' => out.dry_run = true,
                            'q' => out.quiet = true,
                            'i' => return Ok(Command::Info),
                            'h' => return Ok(Command::Help),
                            'V' => return Ok(Command::Version),
                            'j' => {
                                let rest: String = letters[index + 1..].iter().collect();
                                let value = if rest.is_empty() {
                                    take_value(inline.clone())?
                                } else {
                                    rest
                                };
                                out.jobs = parse_jobs(&value)?;
                                // An option that takes a value consumes the rest
                                // of the cluster: `-j4x` is not `-j 4 -x`.
                                break;
                            }
                            other => {
                                return Err(Error::usage(format!("unknown option `-{other}`")))
                            }
                        }
                        index += 1;
                    }
                    continue;
                }
                return Err(Error::usage(format!("unknown option `{name}`")));
            }
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

fn parse_jobs(value: &str) -> Result<usize> {
    let jobs = value
        .parse::<usize>()
        .map_err(|_| Error::usage(format!("`--jobs` expects a number, got `{value}`")))?;
    if jobs == 0 {
        return Err(Error::usage("`--jobs` must be at least 1"));
    }
    Ok(jobs)
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
        let args = parse_ok(&["a.png"]);
        assert_eq!(args.paths.len(), 1);
        assert!(!args.recursive);
        assert_eq!(args.max_size, DEFAULT_MAX_SIZE);
    }

    /// #51: the flag exists, takes a byte count, and defaults to "derive it".
    #[test]
    fn memory_budget_flag() {
        assert_eq!(parse_ok(&["a.png"]).memory_budget, DEFAULT_MEMORY_BUDGET);
        assert_eq!(DEFAULT_MEMORY_BUDGET, 0, "0 means: derive from the limits");
        assert_eq!(
            parse_ok(&["--memory-budget", "1048576", "a.png"]).memory_budget,
            1_048_576
        );
        assert_eq!(
            parse_ok(&["--memory-budget=1048576", "a.png"]).memory_budget,
            1_048_576
        );
        assert!(HELP.contains("--memory-budget"), "the flag belongs in --help");
        let bad: Vec<OsString> = ["--memory-budget", "lots", "a.png"]
            .iter()
            .map(OsString::from)
            .collect();
        assert!(
            parse(bad.into_iter()).is_err(),
            "a non-numeric budget is a usage error, not a zero"
        );
    }

    #[test]
    fn flags_and_values() {
        let args = parse_ok(&[
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
        let args = parse_ok(&["--jobs=3", "--", "--weird-name.png"]);
        assert_eq!(args.jobs, 3);
        assert_eq!(args.paths, vec![PathBuf::from("--weird-name.png")]);
    }

    #[test]
    fn formats_flag_narrows_and_enables_jpeg() {
        let args = parse_ok(&["--formats", "jpeg,png", "x.jpg"]);
        assert!(args.allow.jpeg && args.allow.png);
        assert!(!args.allow.zip && !args.allow.ico && !args.allow.gzip);
        // JPEG is off unless it is named.
        let plain = parse_ok(&["x.jpg"]);
        assert!(!plain.allow.jpeg && plain.allow.png);
    }

    #[test]
    fn rejects_unknown_format_name() {
        let args: Vec<OsString> = ["--formats", "jpeg,bmp", "x"]
            .iter()
            .map(OsString::from)
            .collect();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn rejects_unknown_option() {
        let args: Vec<OsString> = ["--nope", "x"].iter().map(OsString::from).collect();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn rejects_missing_paths() {
        let args: Vec<OsString> = Vec::new();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn rejects_zero_jobs() {
        let args: Vec<OsString> = ["--jobs", "0", "x"].iter().map(OsString::from).collect();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn clustered_short_flags_expand() {
        let args = parse_ok(&["-rq", "dir"]);
        assert!(args.recursive && args.quiet);
        let args = parse_ok(&["-rn", "dir"]);
        assert!(args.recursive && args.dry_run);
        // Order inside the cluster does not matter.
        let args = parse_ok(&["-qr", "dir"]);
        assert!(args.recursive && args.quiet);
    }

    #[test]
    fn attached_short_values_expand() {
        assert_eq!(parse_ok(&["-j4", "x"]).jobs, 4);
        assert_eq!(parse_ok(&["-j", "4", "x"]).jobs, 4);
        assert_eq!(parse_ok(&["-j=4", "x"]).jobs, 4);
        // A value consumes the rest of the cluster: `-j4x` is not `-j4 -x`.
        let args: Vec<OsString> = ["-j4x", "x"].iter().map(OsString::from).collect();
        assert!(parse(args.into_iter()).is_err());
    }

    #[test]
    fn an_unknown_letter_in_a_cluster_is_named() {
        let args: Vec<OsString> = ["-rz", "dir"].iter().map(OsString::from).collect();
        match parse(args.into_iter()) {
            Err(Error::Usage(message)) => assert!(message.contains("-z"), "{message}"),
            other => panic!("expected a usage error, got {other:?}"),
        }
    }

    #[test]
    fn colour_is_a_three_word_choice() {
        assert_eq!(parse_ok(&["x"]).color, ColorChoice::Auto);
        assert_eq!(
            parse_ok(&["--color=always", "x"]).color,
            ColorChoice::Always
        );
        assert_eq!(
            parse_ok(&["--color", "never", "x"]).color,
            ColorChoice::Never
        );
        assert_eq!(parse_ok(&["--no-color", "x"]).color, ColorChoice::Never);
        let args: Vec<OsString> = ["--color=sometimes", "x"]
            .iter()
            .map(OsString::from)
            .collect();
        match parse(args.into_iter()) {
            Err(Error::Usage(message)) => {
                assert!(message.contains("auto, always or never"), "{message}")
            }
            other => panic!("expected a usage error, got {other:?}"),
        }
    }

    #[test]
    fn progress_is_on_unless_turned_off() {
        assert!(!parse_ok(&["x"]).no_progress);
        assert!(parse_ok(&["--no-progress", "x"]).no_progress);
    }

    #[test]
    fn help_and_version() {
        let args: Vec<OsString> = ["--help"].iter().map(OsString::from).collect();
        assert!(matches!(parse(args.into_iter()), Ok(Command::Help)));
        let args: Vec<OsString> = ["--version"].iter().map(OsString::from).collect();
        assert!(matches!(parse(args.into_iter()), Ok(Command::Version)));
    }

    fn command(items: &[&str]) -> Command {
        let args: Vec<OsString> = items.iter().map(OsString::from).collect();
        parse(args.into_iter()).expect("parse should succeed")
    }

    #[test]
    fn a_bare_path_is_the_whole_command() {
        let args = parse_ok(&["photo.png"]);
        assert_eq!(args.paths, vec![PathBuf::from("photo.png")]);

        let args = parse_ok(&["a.png", "b.gz", "-r"]);
        assert!(args.recursive);
        assert_eq!(args.paths.len(), 2);
    }

    #[test]
    fn the_old_verb_is_still_accepted() {
        // 0.1-0.4 documentation and scripts say `tzim optimize <path>`.
        let args = parse_ok(&["optimize", "photo.png"]);
        assert_eq!(args.paths, vec![PathBuf::from("photo.png")]);
        assert!(parse([OsString::from("optimize")].into_iter()).is_err());
    }

    #[test]
    fn information_commands_do_not_need_paths() {
        assert!(matches!(command(&["-i"]), Command::Info));
        assert!(matches!(command(&["--info"]), Command::Info));
        assert!(matches!(command(&["--check-deps"]), Command::CheckDeps));
        assert!(matches!(command(&["--list-formats"]), Command::Formats));
        assert!(matches!(command(&["formats"]), Command::Formats));
    }

    #[test]
    fn a_path_that_starts_with_a_dash_needs_a_separator() {
        let args = parse_ok(&["--", "-weird-name.png"]);
        assert_eq!(args.paths, vec![PathBuf::from("-weird-name.png")]);
    }
}
