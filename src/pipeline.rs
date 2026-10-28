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
    pub fits_tile: bool,
    pub min_size: u64,
    pub max_size: u64,
    /// The most any one job may be expected to need, in bytes (issue #51).
    pub memory_budget: u64,
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
            fits_tile: args.fits_tile,
            min_size: args.min_size,
            max_size: args.max_size,
            memory_budget: budget_or_derived(args),
            allow_hardlinks: args.allow_hardlinks,
            allow_root_ownership_change: args.allow_root_ownership_change,
            no_fsync: args.no_fsync,
        }
    }
}

/// The budget when the user has not given one: the sum of every worst case the
/// other ceilings allow, so that the defaults never skip a file that they admit.
///
/// `4 × --max-size` is the widest per-file term (`2 × len` for the file and the
/// candidate, plus `4 × len` for a TIFF strip, and the larger of the two wins),
/// `8 × --max-pixels` is a PNG's two RGBA buffers, and `--max-uncompressed` is
/// a ZIP entry. The sum is therefore an upper bound on [`worst_case_bytes`] for
/// every format, under any `len ≤ --max-size`. It is deliberately not a claim
/// about how much RAM the machine has: that is what `--memory-budget` is for.
pub fn derived_memory_budget(max_size: u64, max_pixels: u64, max_uncompressed: u64) -> u64 {
    max_size
        .saturating_mul(4)
        .saturating_add(max_pixels.saturating_mul(8))
        .saturating_add(max_uncompressed)
}

/// `--memory-budget`, or the derived ceiling when it is zero.
fn budget_or_derived(args: &Args) -> u64 {
    if args.memory_budget == 0 {
        derived_memory_budget(args.max_size, args.max_pixels, args.max_uncompressed)
    } else {
        args.memory_budget
    }
}

/// What one file may cost, in bytes, from what is cheap to know before reading it
/// (issue #51, decision O2-D).
///
/// The terms, and why each is the size it is:
///
/// | Term | Worst case | Why this number |
/// | --- | --- | --- |
/// | the input, held in memory | `len` | `fs::read` |
/// | the candidate, read back for verification | `len` | a candidate is never larger |
/// | a decoded PNG | `8 × pixels` | two RGBA buffers, one per side |
/// | a decoded JPEG | `7 × pixels` | the coefficient planes plus one decoded frame — the independent decoder hashes one side at a time (§2.6) |
/// | a decoded TIFF strip | `4 × len` | the strip is bounded by the file, and `--max-size` bounds that |
/// | two FITS tile buffers | `2 × len` | the file and its candidate (`2 × len`) plus the row a tile holds and the gzip member written from it; a tile is a row of the array, so the file bounds each |
/// | a decompressed ZIP entry | `--max-uncompressed` | the one term that is not bounded by the file at all, and the reason a budget cannot default to something small |
///
/// `pixels` is the real count when the header carried it (a PNG does) and
/// `--max-pixels` otherwise: the configured ceiling is the bound the rest of the
/// program already refuses to exceed. gzip and zlib are absent on purpose —
/// they are inflated in 64 KiB blocks and never materialise the payload.
///
/// The function takes its ceilings rather than a `Config` so that a test can
/// compute the same number the pipeline will.
pub fn worst_case_bytes(
    len: u64,
    format: Option<detect::Format>,
    pixels: Option<u64>,
    max_pixels: u64,
    max_uncompressed: u64,
) -> u64 {
    let base = len.saturating_mul(2);
    let pixels = pixels.unwrap_or(max_pixels).min(max_pixels);
    let image = match format {
        Some(detect::Format::Png) => pixels.saturating_mul(8),
        Some(detect::Format::Jpeg) => pixels.saturating_mul(7),
        Some(detect::Format::Tiff) => len.saturating_mul(4),
        // The file, its candidate, and two tile-sized buffers — the row a tile
        // holds and the gzip member written from it. A tile is a row of the
        // array, so the file bounds each of them.
        Some(detect::Format::Fits) => len.saturating_mul(2),
        _ => 0,
    };
    let container = match format {
        Some(detect::Format::Zip) => max_uncompressed,
        _ => 0,
    };
    base.saturating_add(image).saturating_add(container)
}

#[cfg(feature = "fits")]
type Confirmation<'a> = Option<&'a mut dyn FnMut(&Path) -> bool>;
#[cfg(not(feature = "fits"))]
type Confirmation<'a> = std::marker::PhantomData<&'a ()>;

/// Processes one file, following the order fixed in docs/safety.md.
///
/// Every early return leaves the original untouched; the only step that changes
/// what is on disk is the final rename inside `replace::commit`.
pub fn process(path: &Path, cfg: &Config) -> Outcome {
    #[cfg(feature = "fits")]
    let confirm: Confirmation<'_> = None;
    #[cfg(not(feature = "fits"))]
    let confirm: Confirmation<'_> = std::marker::PhantomData;
    process_inner(path, cfg, confirm)
}

/// Processes a single top-level file with a confirmation callback that is
/// invoked only after a FITS tile candidate has passed the ordinary verifier.
/// Callers must keep this on the main thread and use it only for an interactive
/// single-file invocation; batch and nested work use `process` without a prompt.
#[cfg(feature = "fits")]
pub fn process_with_confirmation(
    path: &Path,
    cfg: &Config,
    confirm: &mut dyn FnMut(&Path) -> bool,
) -> Outcome {
    process_inner(path, cfg, Some(confirm))
}

#[allow(unused_mut)]
fn process_inner(path: &Path, cfg: &Config, mut confirm: Confirmation<'_>) -> Outcome {
    let started = Instant::now();
    let mut outcome = Outcome::new(path.to_path_buf(), Status::Skipped, 0);
    let result = run(path, cfg, &mut outcome, &mut confirm);
    if let Err(e) = result {
        match e {
            Error::Limit(message) | Error::Refused(message) => {
                outcome.status = Status::Skipped;
                outcome.note = Some(message);
            }
            // A handler that proves the detector wrong is not a failure: the
            // file is simply not the format the signature suggested. The note
            // says which handler and why, because the user will want to know
            // that a `.zlib` of theirs was read and put back untouched.
            Error::Unsupported(message) => {
                outcome.status = Status::Unsupported;
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

fn run(
    path: &Path,
    cfg: &Config,
    outcome: &mut Outcome,
    confirm: &mut Confirmation<'_>,
) -> Result<()> {
    #[cfg(not(feature = "fits"))]
    let _ = confirm;

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

    let probe = detect::probe_path(path)?;
    let format = match probe.format {
        Some(f) => f,
        None => {
            // No note: the status word already carries the whole message, and
            // "unsupported: unsupported format" was a wart in every line and
            // every JSON record.
            outcome.status = Status::Unsupported;
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
                if built.is_empty() {
                    "none".to_string()
                } else {
                    built.join(", ")
                }
            ));
        } else {
            outcome.note = Some(format!(
                "{} is not enabled for this run (use --formats; enabled: {})",
                format.name(),
                if enabled.is_empty() {
                    "none".to_string()
                } else {
                    enabled.join(", ")
                }
            ));
        }
        return Ok(());
    }

    // The budget is checked before the file is read, which is the whole point:
    // a run that is going to be killed by the OOM killer must be skipped with a
    // reason instead. `skipped`, not `failed` — a configured bound is not an
    // error, and the file is untouched either way.
    let cost = worst_case_bytes(
        snapshot.len,
        Some(format),
        probe.pixels,
        cfg.max_pixels,
        cfg.max_uncompressed,
    );
    if cost > cfg.memory_budget {
        outcome.note = Some(format!(
            "worst case {cost} bytes of memory exceeds --memory-budget ({}); \
             lower --max-pixels or --max-uncompressed, raise the budget, or use fewer --jobs",
            cfg.memory_budget
        ));
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
        fits_tile: cfg.fits_tile,
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

    // The same rule the verifier applies, from the same function: with
    // `--strip-metadata` an equal-size candidate is the requested result when
    // the change is a removed metadata field (guarantees I2).
    if !formats::size_ok(snapshot.len as usize, written as usize, cfg.strip_metadata) {
        return Err(Error::internal(
            "a candidate was accepted that is larger than the original",
        ));
    }

    let candidate = fs::read(temp.path())?;
    formats::verify(format, &input, &candidate, &options)?;

    if cfg.dry_run {
        outcome.after = written;
        outcome.status = Status::WouldOptimize;
        return Ok(());
    }

    // For an interactive `--fits-tile` invocation the caller asks only now:
    // after eligibility, candidate creation, and the complete normal verifier.
    // A decline returns before the snapshot check and atomic install; `TempFile`
    // removes the staged candidate on drop, and the original-size outcome is
    // retained because no bytes were written.
    #[cfg(feature = "fits")]
    if format == detect::Format::Fits && cfg.fits_tile {
        if let Some(confirm) = confirm.as_deref_mut() {
            if !confirm(path) {
                outcome.status = Status::Skipped;
                outcome.note =
                    Some("FITS tile rewrite was not confirmed; the file is unchanged".to_string());
                return Ok(());
            }
        }
    }

    outcome.after = written;

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
            fits_tile: false,
            min_size: 0,
            max_size: 2 * 1024 * 1024 * 1024,
            memory_budget: derived_memory_budget(
                2 * 1024 * 1024 * 1024,
                100_000_000,
                8 * 1024 * 1024 * 1024,
            ),
            allow_hardlinks: false,
            allow_root_ownership_change: false,
            no_fsync: true,
        }
    }

    #[cfg(feature = "fits")]
    fn sample_fits_image() -> Vec<u8> {
        sample_fits_image_with_extra(&[])
    }

    #[cfg(feature = "fits")]
    fn sample_fits_image_with_extra(extra: &[&str]) -> Vec<u8> {
        const RECORD: usize = 2880;
        let cards = [
            "SIMPLE  =                    T",
            "BITPIX  =                    8",
            "NAXIS   =                    2",
            "NAXIS1  =                  512",
            "NAXIS2  =                  512",
            "EXTEND  =                    T",
        ];
        let mut file = Vec::new();
        for text in cards
            .into_iter()
            .chain(extra.iter().copied())
            .chain(std::iter::once("END"))
        {
            let mut card = [b' '; 80];
            card[..text.len()].copy_from_slice(text.as_bytes());
            file.extend_from_slice(&card);
        }
        file.resize(file.len().div_ceil(RECORD) * RECORD, b' ');
        file.extend((0..512 * 512).map(|index| ((index / 4) % 251) as u8));
        file.resize(file.len().div_ceil(RECORD) * RECORD, 0);
        file
    }

    #[cfg(feature = "fits")]
    fn fits_options(cfg: &Config) -> Options {
        Options {
            zopfli: cfg.zopfli,
            max_pixels: cfg.max_pixels,
            max_uncompressed: cfg.max_uncompressed,
            max_ratio: cfg.max_ratio,
            strip_metadata: cfg.strip_metadata,
            nested: cfg.nested,
            max_depth: cfg.max_depth,
            depth: 0,
            allow: cfg.allow,
            fits_tile: cfg.fits_tile,
        }
    }

    #[cfg(feature = "fits")]
    fn fits_test_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tzim-fits-confirm-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(feature = "fits")]
    fn enable_fits_tile(cfg: &mut Config) {
        cfg.fits_tile = true;
        cfg.allow.fits = true;
    }

    #[cfg(feature = "fits")]
    #[test]
    fn declining_verified_fits_candidate_preserves_original_and_cleans_temp() {
        let dir = fits_test_dir("no");
        let path = dir.join("eligible.fits");
        let original = sample_fits_image();
        fs::write(&path, &original).unwrap();
        let mut cfg = config();
        enable_fits_tile(&mut cfg);
        let options = fits_options(&cfg);
        let mut prompted = false;
        let mut confirm = |candidate_path: &Path| {
            assert_eq!(candidate_path, path.as_path());
            let staged = fs::read_dir(&dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|entry| {
                    entry
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".tzim-")
                })
                .expect("candidate must be staged before confirmation");
            let candidate = fs::read(staged).unwrap();
            formats::verify(detect::Format::Fits, &original, &candidate, &options)
                .expect("the candidate must pass the ordinary verifier before prompting");
            assert!(candidate.len() < original.len());
            prompted = true;
            false
        };

        let outcome = process_with_confirmation(&path, &cfg, &mut confirm);
        assert!(prompted, "the eligible, verified candidate must prompt");
        assert_eq!(outcome.status, Status::Skipped);
        assert_eq!(outcome.before, original.len() as u64);
        assert_eq!(outcome.after, original.len() as u64);
        assert!(outcome.note.as_deref().unwrap().contains("not confirmed"));
        assert_eq!(fs::read(&path).unwrap(), original);
        let remaining: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(remaining, vec![path]);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(feature = "fits")]
    #[test]
    fn affirmative_confirmation_commits_only_the_verified_candidate() {
        let dir = fits_test_dir("yes");
        let path = dir.join("eligible.fits");
        let original = sample_fits_image();
        fs::write(&path, &original).unwrap();
        let mut cfg = config();
        enable_fits_tile(&mut cfg);
        let options = fits_options(&cfg);
        let mut prompted = false;
        let mut confirm = |_candidate_path: &Path| {
            prompted = true;
            true
        };

        let outcome = process_with_confirmation(&path, &cfg, &mut confirm);
        assert!(prompted);
        assert_eq!(outcome.status, Status::Optimized);
        let candidate = fs::read(&path).unwrap();
        assert!(candidate.len() < original.len());
        formats::verify(detect::Format::Fits, &original, &candidate, &options)
            .expect("the installed file must be the fully verified candidate");
        let remaining: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(remaining, vec![path]);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(feature = "fits")]
    #[test]
    fn unknown_fits_like_content_does_not_reach_confirmation() {
        let dir = fits_test_dir("unknown");
        let path = dir.join("unknown.fits");
        let original = b"not a FITS file".to_vec();
        fs::write(&path, &original).unwrap();
        let mut cfg = config();
        enable_fits_tile(&mut cfg);
        let mut prompted = false;
        let mut confirm = |_candidate_path: &Path| {
            prompted = true;
            true
        };

        let outcome = process_with_confirmation(&path, &cfg, &mut confirm);
        assert!(!prompted);
        assert_eq!(outcome.status, Status::Unsupported);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(all(feature = "fits", unix))]
    #[test]
    fn hardlinked_fits_is_skipped_without_confirmation_by_default() {
        let dir = fits_test_dir("hardlink");
        let path = dir.join("eligible.fits");
        let link = dir.join("alias.fits");
        let original = sample_fits_image();
        fs::write(&path, &original).unwrap();
        fs::hard_link(&path, &link).unwrap();
        let mut cfg = config();
        enable_fits_tile(&mut cfg);
        let mut prompted = false;
        let mut confirm = |_candidate_path: &Path| {
            prompted = true;
            true
        };

        let outcome = process_with_confirmation(&path, &cfg, &mut confirm);
        assert!(!prompted);
        assert_eq!(outcome.status, Status::Skipped);
        assert!(outcome.note.as_deref().unwrap().contains("hard link"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(&link).unwrap(), original);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(feature = "fits")]
    #[test]
    fn refused_checksummed_fits_does_not_reach_confirmation() {
        let dir = fits_test_dir("refused");
        let path = dir.join("checksummed.fits");
        let original = sample_fits_image_with_extra(&["CHECKSUM= 'ABCDEFGH'"]);
        fs::write(&path, &original).unwrap();
        let mut cfg = config();
        enable_fits_tile(&mut cfg);
        let mut prompted = false;
        let mut confirm = |_candidate_path: &Path| {
            prompted = true;
            true
        };

        let outcome = process_with_confirmation(&path, &cfg, &mut confirm);
        assert!(!prompted);
        assert_eq!(outcome.status, Status::Skipped);
        assert!(outcome.note.as_deref().unwrap().contains("CHECKSUM"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(feature = "fits")]
    #[test]
    fn corrupt_fits_header_does_not_reach_confirmation_and_cleans_temp() {
        let dir = fits_test_dir("failed");
        let path = dir.join("corrupt.fits");
        let mut original = vec![b' '; 2880];
        let first = b"SIMPLE  =                    T";
        original[..first.len()].copy_from_slice(first);
        fs::write(&path, &original).unwrap();
        let mut cfg = config();
        enable_fits_tile(&mut cfg);
        let mut prompted = false;
        let mut confirm = |_candidate_path: &Path| {
            prompted = true;
            true
        };

        let outcome = process_with_confirmation(&path, &cfg, &mut confirm);
        assert!(!prompted);
        assert_eq!(outcome.status, Status::Failed);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    /// #51: the estimator is arithmetic over what is cheap to know, and every
    /// term has to be visible in the answer — a term that is silently dropped is
    /// a budget that silently lies.
    #[test]
    fn the_estimator_counts_every_term() {
        let (max_pixels, max_uncompressed) = (100_000_000, 8 * 1024 * 1024 * 1024);

        // The input and the candidate read back for verification.
        assert_eq!(
            worst_case_bytes(1000, None, None, max_pixels, max_uncompressed),
            2000
        );

        // A PNG: the two RGBA buffers dominate a small file, and the declared
        // dimensions are used when the header carried them.
        assert_eq!(
            worst_case_bytes(
                1000,
                Some(detect::Format::Png),
                Some(4_000_000),
                max_pixels,
                0
            ),
            2000 + 8 * 4_000_000
        );
        // Without them, `--max-pixels` is the bound — the ceiling the program
        // already refuses to exceed.
        assert_eq!(
            worst_case_bytes(1000, Some(detect::Format::Png), None, max_pixels, 0),
            2000 + 8 * max_pixels
        );
        // A header cannot raise the ceiling: a lying IHDR is still clamped.
        assert_eq!(
            worst_case_bytes(
                1000,
                Some(detect::Format::Png),
                Some(u64::MAX),
                max_pixels,
                0
            ),
            2000 + 8 * max_pixels
        );

        // A JPEG decodes to the coefficient planes plus one frame.
        assert_eq!(
            worst_case_bytes(
                10,
                Some(detect::Format::Jpeg),
                Some(1_000_000),
                max_pixels,
                0
            ),
            20 + 7 * 1_000_000
        );

        // A TIFF strip is bounded by the file, not by a pixel count.
        assert_eq!(
            worst_case_bytes(500, Some(detect::Format::Tiff), None, max_pixels, 0),
            1000 + 4 * 500
        );

        // A container is charged its decompression ceiling: the one term that a
        // file length cannot bound.
        assert_eq!(
            worst_case_bytes(500, Some(detect::Format::Zip), None, max_pixels, 4096),
            1000 + 4096
        );

        // gzip and zlib stream in 64 KiB blocks; nothing is materialised.
        assert_eq!(
            worst_case_bytes(500, Some(detect::Format::Gzip), None, max_pixels, 4096),
            1000
        );

        // FITS holds the file, its candidate and two tile-sized buffers; a tile
        // is a row of the array, so the file bounds both of them.
        assert_eq!(
            worst_case_bytes(500, Some(detect::Format::Fits), None, max_pixels, 4096),
            2000
        );

        // No term overflows: a huge file length stays a number.
        assert_eq!(
            worst_case_bytes(
                u64::MAX,
                Some(detect::Format::Tiff),
                Some(u64::MAX),
                u64::MAX,
                u64::MAX
            ),
            u64::MAX
        );
    }

    /// The default budget must admit everything the other ceilings allow, or the
    /// defaults would silently start skipping files (#51 acceptance criteria).
    #[test]
    fn the_derived_default_admits_every_ceiling_it_is_derived_from() {
        let (max_size, max_pixels, max_uncompressed) =
            (2 * 1024 * 1024 * 1024, 100_000_000, 8 * 1024 * 1024 * 1024);
        let budget = derived_memory_budget(max_size, max_pixels, max_uncompressed);

        for format in [
            detect::Format::Png,
            detect::Format::Jpeg,
            detect::Format::Tiff,
            detect::Format::Zip,
            detect::Format::Fits,
        ] {
            let cost = worst_case_bytes(max_size, Some(format), None, max_pixels, max_uncompressed);
            assert!(
                cost <= budget,
                "{format:?} at the limits costs {cost}, above the derived {budget}"
            );
        }
        // And it is a bound, not a wildly loose number: twice the largest term.
        assert_eq!(budget, 4 * max_size + 8 * max_pixels + max_uncompressed);
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
