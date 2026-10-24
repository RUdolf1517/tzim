use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::cli::Args;
use crate::detect;
use crate::error::{Error, Result};
use crate::formats::{self, Allow, Cand, Options};
use crate::replace::{self, Snapshot, TempFile};
use crate::report::{Outcome, Status};

/// Everything the per-file pipeline needs, copied out of the command line once.
///
/// Two fields are only consulted on Unix, where ownership and link counts are
/// available without extra dependencies.
#[cfg_attr(not(unix), allow(dead_code))]
pub struct Config {
    pub dry_run: bool,
    pub zopfli: bool,
    pub max_pixels: u64,
    pub max_uncompressed: u64,
    pub max_ratio: u64,
    pub strip_metadata: bool,
    pub nested: bool,
    pub max_depth: u32,
    pub allow: Allow,
    pub min_size: u64,
    pub max_size: u64,
    pub allow_hardlinks: bool,
    pub allow_root_ownership_change: bool,
    pub no_fsync: bool,
}

impl Config {
    pub fn from_args(args: &Args) -> Config {
        Config {
            dry_run: args.dry_run,
            zopfli: args.zopfli,
            max_pixels: args.max_pixels,
            max_uncompressed: args.max_uncompressed,
            max_ratio: args.max_ratio,
            strip_metadata: args.strip_metadata,
            nested: args.nested,
            max_depth: args.max_depth,
            allow: args.allow,
            min_size: args.min_size,
            max_size: args.max_size,
            allow_hardlinks: args.allow_hardlinks,
            allow_root_ownership_change: args.allow_root_ownership_change,
            no_fsync: args.no_fsync,
        }
    }
}

/// Processes one file, following the order fixed in docs/safety.md.
///
/// Every early return leaves the original untouched; the only step that changes
/// what is on disk is the final rename inside `replace::commit`.
pub fn process(path: &Path, cfg: &Config) -> Outcome {
    let started = Instant::now();
    let mut outcome = Outcome::new(path.to_path_buf(), Status::Skipped, 0);
    let result = run(path, cfg, &mut outcome);
    if let Err(e) = result {
        match e {
            Error::Limit(message) | Error::Refused(message) => {
                outcome.status = Status::Skipped;
                outcome.note = Some(message);
            }
            other => {
                outcome.status = Status::Failed;
                outcome.error = Some(other);
            }
        }
    }
    outcome.duration = started.elapsed();
    outcome
}

fn run(path: &Path, cfg: &Config, outcome: &mut Outcome) -> Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.file_type().is_symlink() {
        outcome.note = Some("symbolic link".to_string());
        return Ok(());
    }
    if !md.is_file() {
        outcome.note = Some("not a regular file".to_string());
        return Ok(());
    }

    let snapshot = Snapshot::of(&md);
    outcome.before = snapshot.len;
    outcome.after = snapshot.len;

    if snapshot.len < cfg.min_size {
        outcome.note = Some(format!("smaller than --min-size ({})", cfg.min_size));
        return Ok(());
    }
    if snapshot.len > cfg.max_size {
        outcome.note = Some(format!("larger than --max-size ({})", cfg.max_size));
        return Ok(());
    }
    #[cfg(unix)]
    if snapshot.nlink > 1 && !cfg.allow_hardlinks {
        outcome.note = Some("has more than one hard link (use --allow-hardlinks)".to_string());
        return Ok(());
    }
    if snapshot.readonly {
        outcome.note = Some("file is read-only".to_string());
        return Ok(());
    }

    let format = match detect::detect_path(path)? {
        Some(f) => f,
        None => {
            outcome.status = Status::Unsupported;
            outcome.note = Some("unsupported format".to_string());
            return Ok(());
        }
    };
    outcome.format = Some(format);

    if !cfg.allow.permits(format) {
        outcome.status = Status::Skipped;
        let enabled = cfg.allow.names();
        if !formats::compiled(format) {
            let built = formats::compiled_names();
            outcome.note = Some(format!(
                "this build has no {} handler (compiled: {})",
                format.id(),
                if built.is_empty() { "none".to_string() } else { built.join(", ") }
            ));
        } else {
            outcome.note = Some(format!(
                "{} is not enabled for this run (use --formats; enabled: {})",
                format.name(),
                if enabled.is_empty() { "none".to_string() } else { enabled.join(", ") }
            ));
        }
        return Ok(());
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = TempFile::create(parent)?;

    // Ownership check without libc: the temporary file belongs to the current
    // user, so a different owner on the original means we would change it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let original_uid = md.uid();
        let our_uid = temp.file().metadata()?.uid();
        if original_uid != our_uid && !cfg.allow_root_ownership_change {
            outcome.note = Some(
                "owned by another user; replacing it would change ownership \
                 (use --allow-root-ownership-change)"
                    .to_string(),
            );
            return Ok(());
        }
    }

    let options = Options {
        zopfli: cfg.zopfli,
        max_pixels: cfg.max_pixels,
        max_uncompressed: cfg.max_uncompressed,
        max_ratio: cfg.max_ratio,
        strip_metadata: cfg.strip_metadata,
        nested: cfg.nested,
        max_depth: cfg.max_depth,
        depth: 0,
        allow: cfg.allow,
    };

    // The input is read once: handlers work on bytes, which is also what nested
    // processing needs. The size limit above bounds this allocation.
    let input = fs::read(path)?;
    let cand = {
        let file = temp.file();
        formats::optimize(format, &input, file, &options)?
    };

    let written = match cand {
        Cand::NoGain => {
            outcome.status = Status::AlreadyOptimal;
            return Ok(());
        }
        Cand::Written(n) => n,
    };

    if written == 0 || written >= snapshot.len {
        return Err(Error::internal(
            "a candidate was accepted that is not strictly smaller",
        ));
    }

    let candidate = fs::read(temp.path())?;
    formats::verify(format, &input, &candidate, &options)?;

    outcome.after = written;

    if cfg.dry_run {
        outcome.status = Status::WouldOptimize;
        return Ok(());
    }

    if replace::changed_since(path, &snapshot) {
        return Err(Error::Io(std::io::Error::other(
            "the file changed while it was being processed",
        )));
    }

    replace::commit(&mut temp, path, &snapshot, !cfg.no_fsync)?;
    outcome.status = Status::Optimized;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn config() -> Config {
        Config {
            dry_run: false,
            zopfli: false,
            max_pixels: 100_000_000,
            max_uncompressed: 8 * 1024 * 1024 * 1024,
            max_ratio: 1000,
            strip_metadata: false,
            nested: false,
            max_depth: 3,
            allow: Allow::default(),
            min_size: 0,
            max_size: 2 * 1024 * 1024 * 1024,
            allow_hardlinks: false,
            allow_root_ownership_change: false,
            no_fsync: true,
        }
    }

    #[test]
    fn missing_path_is_a_failure() {
        let outcome = process(Path::new("/nonexistent/tzim-test-file.png"), &config());
        assert_eq!(outcome.status, Status::Failed);
    }

    #[test]
    fn unsupported_format_is_reported_without_changes() {
        let dir = std::env::temp_dir().join(format!("tzim-pipe-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.txt");
        fs::write(&path, b"plain text, nothing to optimize").unwrap();
        let before = fs::read(&path).unwrap();

        let outcome = process(&path, &config());
        assert_eq!(outcome.status, Status::Unsupported);
        assert_eq!(fs::read(&path).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_file_is_unsupported() {
        let dir = std::env::temp_dir().join(format!("tzim-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.bin");
        fs::write(&path, b"").unwrap();
        let outcome = process(&path, &config());
        assert_eq!(outcome.status, Status::Unsupported);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_file_is_skipped() {
        let dir = std::env::temp_dir().join(format!("tzim-big-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.png");
        fs::write(&path, vec![0u8; 4096]).unwrap();
        let mut cfg = config();
        cfg.max_size = 100;
        let outcome = process(&path, &cfg);
        assert_eq!(outcome.status, Status::Skipped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths_are_preserved_for_outcomes() {
        let p = PathBuf::from("relative/path.png");
        let outcome = process(&p, &config());
        assert_eq!(outcome.path, p);
    }
}
