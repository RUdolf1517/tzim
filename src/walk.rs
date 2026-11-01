use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::cli::Args;
use crate::detect;
use crate::error::Error;
use crate::report::ReasonCode;

/// Result of turning command-line paths into the list of files to process.
pub struct Scan {
    pub files: Vec<PathBuf>,
    /// Paths deliberately not processed, with a human-readable and stable reason.
    pub skipped: Vec<(PathBuf, String, ReasonCode)>,
    /// Paths that could not be processed at all.
    pub errors: Vec<(PathBuf, Error)>,
}

/// Collects the files named by the arguments.
///
/// Guarantees:
/// - deterministic order (arguments in order, directory entries sorted);
/// - every file appears at most once, even if reachable through several paths;
/// - symbolic links are not followed unless asked;
/// - a failure on one argument never hides the others.
pub fn scan(args: &Args) -> Scan {
    let mut expanded: Vec<PathBuf> = Vec::new();
    let mut problems: Vec<(PathBuf, Error, ReasonCode)> = Vec::new();

    for path in &args.paths {
        detect::expand(
            path,
            args.recursive,
            args.follow_symlinks,
            &mut expanded,
            &mut problems,
            0,
        );
    }

    let mut seen_files: HashSet<Identity> = HashSet::new();
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();

    for path in expanded {
        if !wanted(&path, args) {
            skipped.push((
                path,
                "filtered out by --include/--exclude".to_string(),
                ReasonCode::PathFilteredByIncludeExclude,
            ));
            continue;
        }
        if !seen_files.insert(identity(&path)) {
            // Duplicate reachable through another argument: silently dropped.
            continue;
        }
        files.push(path);
    }

    // Problems reported by the directory walker: missing paths, links, ...
    // Keep reason identifiers beside the internal Error so they never depend
    // on the wording retained for human diagnostics.
    for (path, err, reason_code) in problems {
        match &err {
            Error::Io(_) => errors.push((path, err)),
            _ => skipped.push((path, err.to_string(), reason_code)),
        }
    }

    Scan {
        files,
        skipped,
        errors,
    }
}

fn wanted(path: &Path, args: &Args) -> bool {
    let ext = detect::extension(path).unwrap_or_default();
    if args.exclude.contains(&ext) {
        return false;
    }
    if !args.include.is_empty() && !args.include.contains(&ext) {
        return false;
    }
    true
}

/// File identity used for deduplication.
///
/// On Unix this is `(device, inode)`, so hard links and symlinked paths collapse
/// correctly. On other platforms the canonical path is used.
#[cfg(unix)]
type Identity = (u64, u64);

#[cfg(unix)]
fn identity(path: &PathBuf) -> Identity {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(path) {
        Ok(md) => (md.dev(), md.ino()),
        // A file we cannot stat still has to be reported; give it a unique key.
        Err(_) => (u64::MAX, hash_path(path)),
    }
}

#[cfg(not(unix))]
type Identity = String;

#[cfg(not(unix))]
fn identity(path: &PathBuf) -> Identity {
    match std::fs::canonicalize(path) {
        Ok(p) => p.to_string_lossy().to_ascii_lowercase(),
        Err(_) => path.to_string_lossy().to_ascii_lowercase(),
    }
}

#[allow(dead_code)]
fn hash_path(path: &PathBuf) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_and_exclude_filters() {
        let included = Args {
            include: vec!["png".into()],
            ..Default::default()
        };
        assert!(wanted(Path::new("a.png"), &included));
        assert!(!wanted(Path::new("a.gz"), &included));

        let excluded = Args {
            exclude: vec!["gz".into()],
            ..Default::default()
        };
        assert!(wanted(Path::new("a.png"), &excluded));
        assert!(!wanted(Path::new("a.gz"), &excluded));
    }

    #[test]
    fn exclude_wins_over_include() {
        let args = Args {
            include: vec!["png".into()],
            exclude: vec!["png".into()],
            ..Default::default()
        };
        assert!(!wanted(Path::new("a.png"), &args));
    }
}
