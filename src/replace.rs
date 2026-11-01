use std::fs::{self, File, FileTimes, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A file being written next to its destination, removed on drop unless
/// committed. The guard is the reason a failed run cannot leave garbage behind.
pub struct TempFile {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl TempFile {
    /// Creates a uniquely named temporary file in `dir`.
    ///
    /// The file is created with `create_new`, so an existing file can never be
    /// reused or truncated, and with mode 0600 so it is not world-readable while
    /// it holds a partial candidate.
    pub fn create(dir: &Path) -> io::Result<TempFile> {
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        for attempt in 0..64u32 {
            let name = format!(".tzim-{pid}-{nanos}-{attempt}.tmp");
            let path = dir.join(name);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    return Ok(TempFile {
                        path,
                        file: Some(file),
                        committed: false,
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique temporary file",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("temporary file handle is available until commit")
    }

    /// Marks the guard as satisfied; `commit` calls this on success.
    fn disarm(&mut self) {
        self.committed = true;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Filesystem attributes captured before processing.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub readonly: bool,
    #[cfg(unix)]
    pub mode: u32,
    #[cfg(unix)]
    pub dev: u64,
    #[cfg(unix)]
    pub ino: u64,
    #[cfg(unix)]
    pub nlink: u64,
    #[cfg(not(unix))]
    #[allow(dead_code)]
    pub nlink: u64,
}

impl Snapshot {
    pub fn of(md: &fs::Metadata) -> Snapshot {
        Snapshot {
            len: md.len(),
            modified: md.modified().ok(),
            accessed: md.accessed().ok(),
            readonly: md.permissions().readonly(),
            #[cfg(unix)]
            mode: {
                use std::os::unix::fs::MetadataExt;
                md.mode()
            },
            #[cfg(unix)]
            dev: {
                use std::os::unix::fs::MetadataExt;
                md.dev()
            },
            #[cfg(unix)]
            ino: {
                use std::os::unix::fs::MetadataExt;
                md.ino()
            },
            #[cfg(unix)]
            nlink: {
                use std::os::unix::fs::MetadataExt;
                md.nlink()
            },
            #[cfg(not(unix))]
            nlink: 1,
        }
    }
}

/// True when the file changed since the snapshot was taken.
///
/// Used before the rename so that a file modified by another process while we
/// were working is never replaced.
pub fn changed_since(path: &Path, snap: &Snapshot) -> bool {
    let md = match fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(_) => return true,
    };
    if md.len() != snap.len {
        return true;
    }
    if md.modified().ok() != snap.modified {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if md.dev() != snap.dev || md.ino() != snap.ino {
            return true;
        }
    }
    false
}

/// Windows reports a transient destination handle conflict as
/// ERROR_SHARING_VIOLATION. Retry that code directly. ERROR_ACCESS_DENIED can
/// also mean ordinary permissions, so code 5 is retried only when a separate
/// DELETE-access probe confirms an incompatible open handle.
#[cfg(any(windows, test))]
const WINDOWS_ERROR_SHARING_VIOLATION: i32 = 32;

#[cfg(any(windows, test))]
fn is_windows_sharing_violation(error: &io::Error) -> bool {
    error.raw_os_error() == Some(WINDOWS_ERROR_SHARING_VIOLATION)
}

#[cfg(windows)]
fn is_windows_rename_sharing_conflict(error: &io::Error, destination: &Path) -> bool {
    match error.raw_os_error() {
        Some(WINDOWS_ERROR_SHARING_VIOLATION) => true,
        Some(5) => has_delete_sharing_conflict(destination),
        _ => false,
    }
}

/// Probe the destination for DELETE access with permissive sharing. If another
/// handle disallows DELETE, Windows returns ERROR_SHARING_VIOLATION (32); an
/// ordinary access-control denial remains code 5 and is not treated as a lock.
#[cfg(windows)]
fn has_delete_sharing_conflict(destination: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;

    const DELETE_ACCESS: u32 = 0x0001_0000;
    const SHARE_READ_WRITE_DELETE: u32 = 0x0000_0007;
    let mut options = OpenOptions::new();
    options
        .access_mode(DELETE_ACCESS)
        .share_mode(SHARE_READ_WRITE_DELETE);
    matches!(
        options.open(destination),
        Err(error) if error.raw_os_error() == Some(WINDOWS_ERROR_SHARING_VIOLATION)
    )
}

/// Retry a rename only when the caller proves a sharing conflict, with two
/// bounded retries and a short increasing delay. Compiled for tests on all
/// platforms so the retry budget and error policy can be exercised directly.
#[cfg(any(windows, test))]
fn retry_sharing_violation<F, P>(mut rename: F, mut is_sharing_conflict: P) -> io::Result<()>
where
    F: FnMut() -> io::Result<()>,
    P: FnMut(&io::Error) -> bool,
{
    let mut retries = 0u32;
    loop {
        match rename() {
            Err(error) if retries < 2 && is_sharing_conflict(&error) => {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(25 * u64::from(retries)));
            }
            result => return result,
        }
    }
}

fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        retry_sharing_violation(
            || fs::rename(from, to),
            |error| is_windows_rename_sharing_conflict(error, to),
        )
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

/// Applies the original attributes to the candidate, flushes it, and replaces
/// the destination in a single rename.
///
/// The rename is the only step that changes the visible filesystem state, and it
/// happens last: every earlier failure leaves the original untouched.
pub fn commit(temp: &mut TempFile, dest: &Path, snap: &Snapshot, fsync: bool) -> io::Result<()> {
    commit_with_directory_sync(temp, dest, snap, fsync, sync_dir, |error| {
        let stderr = io::stderr();
        let mut diagnostics = stderr.lock();
        write_directory_sync_warning(error, &mut diagnostics);
    })
}

fn commit_with_directory_sync<S, W>(
    temp: &mut TempFile,
    dest: &Path,
    snap: &Snapshot,
    fsync: bool,
    sync_directory: S,
    mut warn: W,
) -> io::Result<()>
where
    S: FnOnce(&Path) -> io::Result<()>,
    W: FnMut(&io::Error),
{
    if let Some(file) = temp.file.as_mut() {
        apply_attributes(file, snap)?;
        if fsync {
            file.sync_all()?;
        }
    }
    // Close the handle before renaming: required on Windows, harmless elsewhere.
    temp.file.take();

    rename_with_retry(temp.path(), dest)?;
    temp.disarm();

    if fsync {
        if let Err(error) = sync_directory(dest) {
            warn(&error);
        }
    }
    Ok(())
}

fn write_directory_sync_warning(error: &io::Error, diagnostics: &mut dyn Write) {
    let _ = writeln!(
        diagnostics,
        "tzim: warning: replacement completed, but directory sync failed: {error}"
    );
}

fn apply_attributes(file: &File, snap: &Snapshot) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(snap.mode);
        file.set_permissions(perms)?;
    }
    #[cfg(not(unix))]
    {
        let mut perms = file.metadata()?.permissions();
        perms.set_readonly(snap.readonly);
        file.set_permissions(perms)?;
    }

    let mut times = FileTimes::new();
    let mut have_time = false;
    if let Some(m) = snap.modified {
        times = times.set_modified(m);
        have_time = true;
    }
    if let Some(a) = snap.accessed {
        times = times.set_accessed(a);
        have_time = true;
    }
    if have_time {
        file.set_times(times)?;
    }
    Ok(())
}

/// Flushes the directory entry so the rename survives a crash. A failure here
/// is reported as a warning after the successful replacement, not as a failed file.
#[cfg(unix)]
fn sync_dir(dest: &Path) -> io::Result<()> {
    let dir = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dest: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_removes_uncommitted_file() {
        let dir = std::env::temp_dir().join(format!("tzim-guard-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path;
        {
            let temp = TempFile::create(&dir).unwrap();
            path = temp.path().to_path_buf();
            assert!(path.exists());
        }
        assert!(!path.exists(), "uncommitted temporary file must be removed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_roundtrip_detects_changes() {
        let dir = std::env::temp_dir().join(format!("tzim-snap-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.bin");
        fs::write(&path, b"hello").unwrap();
        let snap = Snapshot::of(&fs::symlink_metadata(&path).unwrap());
        assert!(!changed_since(&path, &snap));

        fs::write(&path, b"hello world").unwrap();
        assert!(changed_since(&path, &snap));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn raw_error_32_is_the_unconditional_sharing_violation_code() {
        let sharing = io::Error::from_raw_os_error(WINDOWS_ERROR_SHARING_VIOLATION);
        let access_denied = io::Error::from_raw_os_error(5);
        let generic_permission =
            io::Error::new(io::ErrorKind::PermissionDenied, "permission denied");
        assert!(is_windows_sharing_violation(&sharing));
        assert!(!is_windows_sharing_violation(&access_denied));
        assert!(!is_windows_sharing_violation(&generic_permission));
    }

    #[test]
    fn sharing_violation_gets_at_most_two_retries() {
        let mut attempts = 0;
        let result = retry_sharing_violation(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(io::Error::from_raw_os_error(
                        WINDOWS_ERROR_SHARING_VIOLATION,
                    ))
                } else {
                    Ok(())
                }
            },
            is_windows_sharing_violation,
        );
        assert!(result.is_ok());
        assert_eq!(attempts, 3, "initial attempt plus two retries");

        attempts = 0;
        let result = retry_sharing_violation(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(
                    WINDOWS_ERROR_SHARING_VIOLATION,
                ))
            },
            is_windows_sharing_violation,
        );
        assert_eq!(
            result.unwrap_err().raw_os_error(),
            Some(WINDOWS_ERROR_SHARING_VIOLATION),
        );
        assert_eq!(attempts, 3, "the retry budget must be bounded");
    }

    #[cfg(windows)]
    #[test]
    fn windows_real_sharing_violation_retries_after_the_handle_is_released() {
        use std::process::{Command, Stdio};

        let dir = std::env::temp_dir().join(format!(
            "tzim-sharing-lock-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let destination = dir.join("destination.bin");
        fs::write(&destination, b"original").unwrap();
        let mut temp = TempFile::create(&dir).unwrap();
        temp.file().write_all(b"replacement").unwrap();
        drop(temp.file.take());

        let script = dir.join("hold-without-delete-share.ps1");
        let ready = dir.join("handle-ready");
        let release = dir.join("release-handle");
        fs::write(
            &script,
            r#"param([string]$Path, [string]$Ready, [string]$Release)
$stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
[System.IO.File]::WriteAllText($Ready, "locked")
while (-not [System.IO.File]::Exists($Release)) { Start-Sleep -Milliseconds 2 }
$stream.Dispose()
"#,
        )
        .unwrap();
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script)
            .arg(&destination)
            .arg(&ready)
            .arg(&release)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Windows CI provides powershell.exe");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready.exists() && std::time::Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if !ready.exists() {
            let _ = fs::write(&release, b"release");
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(&dir);
            panic!("PowerShell did not acquire the destination handle");
        }

        let mut attempts = 0;
        let mut first_error_code = None;
        let result = retry_sharing_violation(
            || {
                attempts += 1;
                let result = fs::rename(temp.path(), &destination);
                if attempts == 1 {
                    if let Err(error) = &result {
                        first_error_code = error.raw_os_error();
                    }
                }
                result
            },
            |error| {
                let sharing_conflict = is_windows_rename_sharing_conflict(error, &destination);
                if attempts == 1 && sharing_conflict {
                    let _ = fs::write(&release, b"release");
                }
                sharing_conflict
            },
        );
        let release_result = fs::write(&release, b"release");
        if release_result.is_err() {
            let _ = child.kill();
        }
        let child_status = child.wait().unwrap();
        let destination_bytes = fs::read(&destination).unwrap_or_default();
        let temp_exists = temp.path().exists();
        if result.is_ok() {
            temp.disarm();
        }
        drop(temp);
        let _ = fs::remove_dir_all(&dir);

        assert!(release_result.is_ok(), "could not signal the lock holder");
        assert!(child_status.success(), "PowerShell lock holder failed");
        assert!(
            matches!(
                first_error_code,
                Some(WINDOWS_ERROR_SHARING_VIOLATION) | Some(5)
            ),
            "unexpected Windows sharing error: {first_error_code:?}"
        );
        assert!(result.is_ok(), "rename retry failed: {result:?}");
        assert_eq!(
            attempts, 2,
            "release during the first backoff should allow retry 1"
        );
        assert_eq!(destination_bytes, b"replacement");
        assert!(!temp_exists);
    }

    #[test]
    fn access_denied_is_not_retried() {
        let mut attempts = 0;
        let result = retry_sharing_violation(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(5))
            },
            is_windows_sharing_violation,
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
        assert_eq!(attempts, 1);
    }

    #[cfg(windows)]
    #[test]
    fn access_denied_is_not_retried_without_delete_share_evidence() {
        let dir = std::env::temp_dir().join(format!("tzim-access-denied-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let destination = dir.join("destination.bin");
        fs::write(&destination, b"ordinary writable file").unwrap();
        assert!(!has_delete_sharing_conflict(&destination));

        let mut attempts = 0;
        let result = retry_sharing_violation(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(5))
            },
            |error| is_windows_rename_sharing_conflict(error, &destination),
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
        assert_eq!(attempts, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn post_rename_directory_sync_failures_warn_once_and_keep_success() {
        for (index, message) in ["directory open failed", "directory fsync failed"]
            .into_iter()
            .enumerate()
        {
            let dir =
                std::env::temp_dir().join(format!("tzim-dir-sync-{}-{index}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let dest = dir.join("file.bin");
            fs::write(&dest, b"original").unwrap();
            let snapshot = Snapshot::of(&fs::symlink_metadata(&dest).unwrap());
            let mut temp = TempFile::create(&dir).unwrap();
            temp.file().write_all(b"replacement").unwrap();

            let mut diagnostics = Vec::new();
            let result = commit_with_directory_sync(
                &mut temp,
                &dest,
                &snapshot,
                true,
                |_| Err(io::Error::other(message)),
                |error| write_directory_sync_warning(error, &mut diagnostics),
            );

            assert!(result.is_ok(), "directory sync must not undo a rename");
            assert_eq!(fs::read(&dest).unwrap(), b"replacement");
            let diagnostics = String::from_utf8(diagnostics).unwrap();
            assert_eq!(diagnostics.lines().count(), 1, "{diagnostics:?}");
            assert!(diagnostics.contains("replacement completed, but directory sync failed"));
            assert!(diagnostics.contains(message));
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
