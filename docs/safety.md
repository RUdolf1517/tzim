# Safe file replacement

Related: [guarantees](guarantees.md) · [cli](cli.md) · [testing](testing.md)

Tzim replaces files. That is the most dangerous thing it does, so the replacement procedure is specified
here in full, including what it does *not* guarantee.

The rule is simple: **the original file is the last thing that changes, and it changes in one atomic step.**

---

## 1. Algorithm

For every file, in this order:

1. **Inspect without following links.** `symlink_metadata` (lstat). Refuse symbolic links, directories,
   FIFOs, sockets and devices. A missing path is an error for that argument.
2. **Capture the snapshot from that stat**, without opening the file: length, modification time (with
   nanoseconds), access time, permission bits, hard-link count, `(device, inode)`. It is this snapshot that
   step 10 re-checks.
3. **Refuse hard-linked files** when the link count is greater than one (`--allow-hardlinks` overrides).
   Replacing one of several links would silently separate the paths.
4. **Refuse read-only files.** If no write permission bit is set for the owner (POSIX) or the file is marked
   read-only (Windows), the file is skipped rather than replaced.
5. **Detect the format** by opening the path read-only and reading its first bytes. The file is then read in
   full, once, by the handler in step 7: two opens in total, after a stat that does not open it. No
   handler → `unsupported`, nothing written.
6. **Create the candidate** as a new file in the **same directory**, with `create_new` and mode `0600`, using
   a unique name. A guard object owns it and removes it on any early exit.
7. **Optimize** into the candidate. The handler must not touch the original.
8. **Verify** the candidate against the format contract, decoding both files with the readers named in
   [guarantees §2.6](guarantees.md#26-verification-independence) — an independent decoder where one exists,
   this repository's own reader where it does not, with the difference stated there rather than glossed over.
   This includes checksum validation of the candidate.
9. **Compare sizes.** Unless `candidate.len() < original.len()`, the candidate is discarded and the outcome
   is `already_optimal`.
10. **Re-check the original.** Stat the path again and compare `(device, inode, length, mtime)`. If anything
    changed, abort this file: another process modified it while we were working.
11. **Apply metadata** to the candidate: permission bits, modification time, access time.
12. **Flush.** `fsync` the candidate (unless `--no-fsync`).
13. **Replace.** A single `rename(candidate, original)` in the same directory. On POSIX this is atomic.
14. **Flush the directory** on Unix (`fsync` on the directory handle) so the rename itself is durable.
15. **Report.** The temporary guard is disarmed only after a successful rename.

No step truncates, edits or rewrites the original in place. There is no point in the procedure where the
original is partially written.

---

## 2. Temporary files

- Created in the **destination directory**, never in `/tmp`, so the final `rename` stays within one
  filesystem and remains atomic.
- Names follow `.tzim-<pid>-<nanos>-<attempt>.tmp`: process id, the current time in nanoseconds and an attempt
  counter, so a name is unique in practice without a random number generator. They are created with
  `create_new` (never reuse an existing file) and owned by an RAII guard. On failure, error or early return
  the guard deletes the file; on success the file is not deleted at all — it is `rename`d over the
  destination and the guard is disarmed afterwards.
- The guard does not survive `SIGKILL`, `abort()`, or a power loss. An orphaned temporary file is possible;
  it is always a *new* file, never a damaged original.
- Tzim never deletes files it did not create, and does not scan directories for "stale" temporaries. That
  would require guessing, and guessing is what this project avoids.
- `--dry-run` also creates the candidate (verification and exact sizing require the real bytes), then
  removes it. No existing file is opened for writing in a dry run.

---

## 3. Metadata

| Property | Preserved | How | Notes |
| --- | --- | --- | --- |
| POSIX permission bits | yes | applied to the candidate before `rename` | setuid/setgid bits are copied as-is; they are not "fixed" |
| Modification time (mtime) | yes | `File::set_times` on the candidate | nanosecond precision where the platform supports it |
| Access time (atime) | best effort | captured before reading, restored with the candidate | reading the file may update atime on some mounts |
| Windows read-only attribute | yes | `set_permissions` | a read-only file is skipped, not replaced |
| Owner / group | **no** | — | the new file belongs to the user running Tzim; on setgid directories the group comes from the directory |
| ACLs | **no** | — | POSIX ACLs and Windows DACLs are not copied; `std` offers no portable API |
| Extended attributes, alternate data streams | **no** | — | not copied |
| Creation/birth time, ctime, inode | **no** | — | replacement creates a new inode; this is inherent to atomic replacement |
| Sparse regions, filesystem compression, immutable flags | **no** | — | the candidate is a normal file |

**Known consequence:** if a file's access control matters (for example, a shared file with an ACL, or a file
owned by another user), replacing it changes who can read or write it. Do not run Tzim as root over other
users' files; the ownership would change. This limitation is documented in the README and is not a bug.

---

## 4. Atomicity and durability

| Platform | Replacement | Atomicity |
| --- | --- | --- |
| Linux, macOS, BSD | `rename(2)` | atomic within one filesystem: a concurrent reader sees either the old file or the new one, never a mixture |
| Windows | `std::fs::rename`, which uses `MoveFileExW` with a fallback to `SetFileInformationByHandle` | replaces the target in one call, but the platform documentation gives no crash-atomicity guarantee; an open handle on the target can make the call fail |
| Network filesystems (NFS, SMB) | `rename` | depends on the server; Tzim makes no claim here |
| Any platform, cross-filesystem | — | cannot happen: the candidate is always created in the destination directory |

With the default `fsync` behaviour, after a power loss the file is either the complete old version or the
complete new version, with the rename itself flushed on Unix. Without `--no-fsync` this costs one extra
flush per file; with it, the data may be lost in a crash even though the rename completed.

**What is explicitly not guaranteed:**

- No protection against failing hardware, lying disks, or a filesystem that does not implement its own
  guarantees.
- No transaction spanning several files. Replacing 10 000 files is 10 000 independent atomic operations; an
  interruption leaves some files new and some old, each of them individually valid and verified.
- No atomicity for a whole directory tree.
- No rollback. There are no backups; if you need the original bytes, keep a copy.

---

## 5. Failure modes

Every row ends with the original file intact unless stated otherwise.

| Situation | Behaviour | Status | Exit |
| --- | --- | --- | --- |
| Bad arguments | nothing is processed | — | `2` |
| Path does not exist / permission denied on read | error for that path, others continue | `failed` | `1` |
| Unsupported or unknown format | nothing written | `unsupported` | `0` |
| Valid input that policy declines (signed archive, ZIP64, encryption, multi-member gzip) | nothing written | `skipped` (`refused`) | `0` |
| Malformed file of a supported format | refused, nothing written | `failed` (`parse`) | `1` |
| Limit exceeded (size, pixels, ratio) | skipped, nothing written | `skipped` | `0` |
| Candidate cannot be created (permissions, read-only directory) | error | `failed` (`io`) | `1` |
| Disk full while writing the candidate | temporary file removed | `failed` (`io`) | `1` |
| Verification fails | run aborts immediately | `failed` (`verify`) | `4` |
| Candidate is not smaller | temporary file removed | `already_optimal` | `0` |
| Original changed during processing | operation aborted | `failed` (`internal`/`io`) | `1` |
| `fsync` of the candidate fails | temporary file removed, nothing replaced | `failed` (`io`) | `1` |
| `rename` fails | temporary file removed | `failed` (`io`) | `1` |
| Directory `fsync` fails after a successful rename | warning on stderr | `optimized` | `0` |
| `SIGINT` / `SIGTERM` | process ends, temporary file may remain | — | `130` / `143` |
| Panic (`panic = "abort"`) | process aborts, temporary file may remain | — | `134` on Unix |
| `SIGKILL` | temporary file may remain | — | `137` |

A verification failure is never retried, downgraded or ignored: it means a contract was violated, which is a
bug in Tzim.

---

## 6. Concurrency

- **Several files at once:** each file has its own candidate and its own guard. `--jobs` bounds the number
  of concurrent candidates, which is also the memory bound.
- **Several Tzim processes on the same file:** no lock is taken. Both compute candidates; whichever renames
  last wins, and both outputs are individually valid and verified. The second process usually detects the
  first one's change at step 10 and aborts. The residual window between that check and the `rename` cannot
  be closed without advisory locking, and stale locks are a worse failure mode than the race, so it is
  documented rather than "fixed".
- **External writer:** the same mechanism applies. A file that changes mid-run is not replaced, and the
  user is told so.
- **Windows:** an open handle held by another process can cause the `rename` to fail. That surfaces as a
  normal error with the original intact; it is retried only if the failure is explicitly a sharing
  violation, at most twice, with a short backoff.

---

## 7. Disk space

The candidate needs up to the size of the output file, in the same directory as the original. Tzim does not
pre-check free space (`std` has no portable `statvfs`); it handles `ENOSPC` as an ordinary I/O error: the
temporary file is removed and the original stays untouched.

For very large files on nearly full volumes, free at least the size of the largest file before running.

---

## 8. Privileges

- Running as a normal user over your own files preserves permissions and ownership.
- A file whose owner differs from the user running Tzim is **skipped with a reason**, unless
  `--allow-root-ownership-change` is given: replacing it would change its ownership to whoever ran the tool.
  This is not specific to root — an ordinary user hits the same rule on someone else's file — and the check
  is Unix-only, because ownership is not comparable on other platforms. Nothing is printed but the reason on
  the file's own line.
- Files without write permission are skipped rather than replaced.
- Tzim never elevates privileges, never calls `sudo`, and never changes its own credentials.

---

## 9. Reporting

Every file gets exactly one outcome, reported even when nothing happened. Failures include the operation
that failed and the OS error, for example:

```
photo.png   PNG   failed: cannot create temporary file in "/srv/images": Permission denied (os error 13)
```

Diagnostics go to stderr; results go to stdout. In `--json` mode the same information appears in the `error`
object of the file record.
