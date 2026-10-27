# Post-merge record — PR #73

Date: 2026-10-27.

## Action and identity

The owner explicitly authorized the merge and asked to be informed immediately of any problems. The manager
ran `gh pr merge 73 --merge`; it exited successfully with no error output.

- PR #73 is **MERGED**, merged at `2026-10-27T14:49:33Z`.
- Merge commit on `main`: `a404837b391337dbfbcda86e425e08960a65c782`.
- Parents: baseline `5dd45b38412ffb762a1fc380bd291351c9e6601e` and reviewed PR head
  `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`.
- The merge commit tree is `80e61c2830c4a6ac52bb590b046e06deadbc8a25`, identical to the reviewed head tree.
  The targeted F-11 review therefore covers the same code tree that was merged.

## Post-merge checks

| Run | Result | URL |
| --- | --- | --- |
| Main CI `40524876729` | completed, **success** | https://github.com/RUdolf1517/tzim/actions/runs/40524876729 |
| Main lockfile `40524876718` | completed, **success** | https://github.com/RUdolf1517/tzim/actions/runs/40524876718 |

The run API identifies both with `head_sha=a404837b391337dbfbcda86e425e08960a65c782`. No post-merge failure or
unexpected new commit was observed. The earlier PR HEASARC, fuzz, corpus, third-party and measurement records
remain identified by the reviewed PR head in [EVIDENCE-INVENTORY.md](EVIDENCE-INVENTORY.md); the expected
measurement failure and its open rows are not erased by the merge.

## Hold and scope

This merge is not writer acceptance beyond the previously documented scope, full/stable FITS admission, a tag,
a Release, an evidence-asset publication, an issue closure, or permission for external promotion. The owner's
1A/2C/3A/4A/5C/6B/7A decisions and all known limitations remain in force.

The merged PR contains the coder's code and its committed evidence. Manager documentation changes remain in the
Arena worktree and are not claimed to be in `main` by this record. Before any release, the final source manifest
must name the actual release SHA and account for any later documentation/package commit. Raw Actions ZIP
contents remain uninspected because the downloads ended in `EOF`; API metadata is not content verification.
