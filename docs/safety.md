# Safe file replacement

Related: [guarantees](guarantees.md) · [cli](cli.md) · [testing](testing.md)

Tzim replaces files. That is the most dangerous thing it does, so the replacement procedure is specified
here in full, including what it does *not* guarantee.

The rule is simple: **the original file is the last thing that changes, and it changes in one atomic step.**

---

## 1. Algorithm

For every file, in this order (`src/pipeline.rs::run`, then `src/replace.rs::commit`):

1. **Inspect without following links.** `symlink_metadata` (lstat). A symbolic link is skipped (never
   followed), and anything that is not a regular file — directory, FIFO, socket, device — is skipped.
   A missing path is an error for that path only.
2. **Take the snapshot from that stat, not from an open handle.** Length, modification time, access time,
   permission bits, hard-link count, `(device, inode)`. No file is opened at this point, which is why the
   snapshot describes the name, not a file description.
3. **Apply the size limits** (`--min-size`, `--max-size`). Outside the range the file is skipped.
4. **Refuse hard-linked files** when the link count is greater than one (`--allow-hardlinks` overrides).
   Replacing one of several links would silently separate the paths.
5. **Refuse read-only files.** If the file is marked read-only, it is skipped rather than replaced.
6. **Detect the format** by opening the path read-only and reading its first bytes
   (`detect::detect_path`). No handler for the detected format → `unsupported`, nothing written.
7. **Check the policy.** `--formats` decides which formats this run may touch; a format that is not
   compiled into the build, or not enabled for this run, is `skipped` with the reason.
8. **Create the candidate** as a new file in the **same directory**, with `create_new` and mode `0600`, under
   a unique name. A guard object owns it and removes it on any early return. On Unix, the candidate's owner
   is compared with the original's: a different owner means the replacement would change ownership, so the
   file is skipped unless `--allow-root-ownership-change` is given.
9. **Read the input once** (bounded by `--max-size`) and **optimize** into the candidate. The handler must
   not touch the original.
10. **Compare sizes** — `formats::size_ok`: a candidate that is empty, or not strictly smaller, is not a
    result, with the single documented exception of an equal-size candidate under `--strip-metadata`
    ([guarantees](guarantees.md), I2). A handler that finds nothing to gain returns `already_optimal`
    before this point; a candidate that is *larger* is an internal error, because no handler may produce one.
11. **Verify** the candidate against the format contract, reading the original and the candidate back from
    their files. See [guarantees](guarantees.md); this includes checksum validation of the candidate.
12. **`--dry-run` stops here.** The candidate is deleted by the guard and nothing is replaced.
13. **Re-check the original.** Stat the path again and compare `(device, inode, length, mtime)`. If anything
    changed, abort this file: another process modified it while we were working.
14. **Apply metadata** to the candidate: permission bits, modification time, access time.
15. **Flush** the candidate with `fsync` (unless `--no-fsync`), then close the handle — required before a
    rename on Windows.
16. **Replace.** A single `rename(candidate, original)` in the same directory, then disarm the guard. On
    POSIX this is atomic.
17. **Flush the directory** on Unix (`fsync` on the directory handle, best effort) so the rename itself is
    durable. A failure here is a warning, not a failed file.

No step truncates, edits or rewrites the original in place. There is no point in the procedure where the
original is partially written.

Which of these steps a test actually pins is stated in [testing §2](testing.md#2-traceability), under the
`T-FS-*` ids: steps 1, 3, 4, 10, 11 and 13 for the cases that exist, and the ones that do not — FIFO, socket,
device, hard link, disk full, `SIGKILL`, a file changed mid-run, a symlink loop — are listed there as
untested. The contract in this section is not narrowed by that list; the evidence behind it is.

---

## 2. Temporary files

- Created in the **destination directory**, never in `/tmp`, so the final `rename` stays within one
  filesystem and remains atomic.
- Names follow `.tzim-<pid>-<nanoseconds>-<attempt>.tmp`: the pid separates processes, the clock separates
  two concurrent runs inside one process, and `attempt` (0..64) resolves the collision that remains. They
  are created with `create_new`, so an existing file can never be reused or truncated, and with mode `0600`.
- The guard removes the file on **every early exit** — an error, a refusal, a dry run. On success the file is
  **renamed**, and the guard is disarmed only after the rename returns.
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

The rows marked with an id have a test under that id in [testing §2](testing.md#2-traceability); a row without
one is the intended behaviour and is not yet exercised.

| Situation | Behaviour | Status | Exit | Test |
| --- | --- | --- | --- | --- |
| Bad arguments | nothing is processed | — | `2` | `usage_errors_exit_with_two` |
| Path does not exist / permission denied on read | error for that path, others continue | `failed` | `1` | `T-FS-03` |
| Unsupported or unknown format | nothing written | `unsupported` | `0` | — |
| Valid input that policy declines (signed archive, ZIP64, encryption, multi-member gzip) | nothing written | `skipped` (`refused`) | `0` | — |
| Malformed file of a supported format | refused, nothing written | `failed` (`parse`) | `1` | — |
| Limit exceeded (size, pixels, ratio) | skipped, nothing written | `skipped` | `0` | — |
| Candidate cannot be created (permissions, read-only directory) | error | `failed` (`io`) | `1` | — |
| Disk full while writing the candidate | temporary file removed | `failed` (`io`) | `1` | — |
| Verification fails | run aborts immediately | `failed` (`verify`) | `4` | — |
| Candidate is not smaller | temporary file removed | `already_optimal` | `0` | `already_optimal_zip_is_left_alone` |
| Original changed during processing | operation aborted | `failed` (`internal`/`io`) | `1` | — (`snapshot_roundtrip_detects_changes` covers the predicate, not the race) |
| `fsync` of the candidate fails | temporary file removed, nothing replaced | `failed` (`io`) | `1` | — |
| `rename` fails | temporary file removed | `failed` (`io`) | `1` | — |
| Directory `fsync` fails after a successful rename | warning on stderr | `optimized` | `0` | — |
| `SIGINT` / `SIGTERM` | process ends, temporary file may remain | — | `130` / `143` | — |
| Panic (`panic = "abort"`) | process aborts, temporary file may remain | — | `134` on Unix | — |
| `SIGKILL` | temporary file may remain | — | `137` | — |

A verification failure is never retried, downgraded or ignored: it means a contract was violated, which is a
bug in Tzim.

---

## 6. Concurrency

- **Several files at once:** each file has its own candidate and its own guard. `--jobs` bounds the number
  of concurrent candidates, which is also the memory bound.
- **Several Tzim processes on the same file:** no lock is taken. Both compute candidates; whichever renames
  last wins, and both outputs are individually valid and verified. The second process usually detects the
  first one's change at step 13 and aborts. The residual window between that check and the `rename` cannot
  be closed without advisory locking, and stale locks are a worse failure mode than the race, so it is
  documented rather than "fixed". The same window exists between the format probe (step 6), the full read
  (step 9) and the check — three separate opens of the path — and it is closed only as far as
  `(device, inode, length, mtime)` can detect.
- **External writer:** the same mechanism applies. A file that changes mid-run is not replaced, and the
  user is told so.
- **Windows:** an open handle held by another process can cause the `rename` to fail. That surfaces as a
  normal error with the original intact; it is retried only if the failure is explicitly a sharing
  violation, at most twice, with a short backoff.

---

## 7. Disk space and memory

The candidate needs up to the size of the output file, in the same directory as the original. Tzim does not
pre-check free space (`std` has no portable `statvfs`); it handles `ENOSPC` as an ordinary I/O error: the
temporary file is removed and the original stays untouched.

For very large files on nearly full volumes, free at least the size of the largest file before running.

### 7.1 Memory — an enforced budget

`--jobs` bounds the number of files in flight. It does **not** bound memory: four workers each holding a
few-hundred-megabyte image is a legitimate `--jobs 4`. Bytes are bounded by `--memory-budget` instead
(issue #51, decision O2-D).

What a single file can cost, and what the guard therefore charges for it:

| Item | Worst case | Charged |
| --- | --- | --- |
| the original, held in memory | `--max-size`, 2 GiB by default (`fs::read`) | `len` |
| the candidate, read back for verification | up to the same | `len` |
| a decoded PNG image, both sides | `8 × pixels` | the `IHDR` count when the header carries it, `--max-pixels` otherwise |
| a decoded JPEG image, both sides | `7 × pixels` | `--max-pixels` |
| a decoded TIFF strip | `4 × len` | `len` |
| a decompressed ZIP entry | `--max-uncompressed`, 8 GiB by default | `--max-uncompressed` |

The PNG and JPEG charges include verification. For JPEG, `7 × pixels` remains the #51 accounting term and
includes the independent `jpeg-decoder` frame: each side is decoded, CRC-32 hashed and dropped before the other
side is decoded. After the exact local coefficient comparison, both coefficient-plane sets are explicitly
dropped before V1 starts, so they do not overlap that frame. No two independent full-frame pixel buffers are
live at once. gzip and zlib are absent because they are inflated in 64 KiB blocks and never materialise the
payload.

The check happens **before the file is read**. A file whose charge exceeds the budget is `skipped`, with a
reason naming the flag, and it is never opened by the pipeline — so an input that would have been killed by
the OOM killer is now a line of output and exit code `0`. `skipped`, not `failed`: a configured bound is not
an error, and the file is untouched.

The default is derived from the other ceilings:

```
--memory-budget = 4 × --max-size + 8 × --max-pixels + --max-uncompressed
```

which is an upper bound on every row above, so the defaults never skip a file that `--max-size`,
`--max-pixels` and `--max-uncompressed` already admit. That makes the default useless as protection against
the 29d00c7 numbers — **set it to what the machine actually has**:

```console
$ tzim --jobs 4 --memory-budget 2147483648 ~/Photos
```

The budget is enforced **per file**, not for the run: with `--jobs N`, up to `N` files are charged at once, so
`N × budget` is the ceiling to compare against RAM. Lower `--jobs` along with the budget when memory is
tight, and lower `--max-pixels` for images and `--max-uncompressed` for containers if a single file is the
problem. The arithmetic is a comparison of integers; it costs no dependency and no measurable binary size.

---

## 8. Privileges

- Running as a normal user over your own files preserves permissions and ownership.
- Replacing a file that belongs to **another user** changes its ownership: the candidate belongs to whoever
  runs Tzim. There is no portable way to keep the original owner without libc, so on Unix Tzim compares the
  candidate's uid with the original's and **skips** such a file, with a reason that names
  `--allow-root-ownership-change`, the flag that permits the replacement anyway. Nothing is printed unless
  the file is actually skipped; the check is not about root specifically, but root is the common case.
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
