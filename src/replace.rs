use std::fs::{self, File, FileTimes, OpenOptions};
use std::io;
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

/// Applies the original attributes to the candidate, flushes it, and replaces
/// the destination in a single rename.
///
/// The rename is the only step that changes the visible filesystem state, and it
/// happens last: every earlier failure leaves the original untouched.
pub fn commit(temp: &mut TempFile, dest: &Path, snap: &Snapshot, fsync: bool) -> io::Result<()> {
    if let Some(file) = temp.file.as_mut() {
        apply_attributes(file, snap)?;
        if fsync {
            file.sync_all()?;
        }
    }
    // Close the handle before renaming: required on Windows, harmless elsewhere.
    temp.file.take();

    fs::rename(temp.path(), dest)?;
    temp.disarm();

    if fsync {
        sync_dir(dest);
    }
    Ok(())
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

/// Flushes the directory entry so the rename survives a crash. Best effort: a
/// failure here is reported as a warning by the caller, not as a failed file.
#[cfg(unix)]
fn sync_dir(dest: &Path) {
    if let Some(dir) = dest.parent() {
        if let Ok(handle) = File::open(dir) {
            let _ = handle.sync_all();
        }
    }
}

#[cfg(not(unix))]
fn sync_dir(_dest: &Path) {}

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
}
