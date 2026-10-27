# Owner decisions for the next release scope

Date: 2026-10-27. The owner answered the seven release-scope questions as
**1A, 2C, 3A, 4A, 5C, 6B, 7A**. These decisions distinguish a general release containing opt-in FITS code
from full PDF/FITS format admission. They do not themselves accept a PR, close an issue, create a Release,
or authorise publication.

| Question | Owner choice | Binding interpretation |
| --- | --- | --- |
| 1. Release/FITS/#70 scope | **1A — general release, FITS opt-in** | #70 tracks implementation. FITS remains behind `--fits-tile`; implementation is not full format admission or a stability claim. |
| 2. Historical FITS mismatch-stop violation | **2C — do not use for release claims** | Retain the historical material, but do not use it for acceptance or release/performance claims and do not let it block the general opt-in release. It is not accepted as a revised pre-registration. |
| 3. Current published-vs-current differences | **3A — investigate first** | The five FITS input-byte and four PDF result-table differences must be investigated before the corresponding full PDF/FITS format acceptance. Baselines and thresholds stay unchanged. In combination with 1A, this does not claim the differences are resolved or accepted for the general opt-in release. |
| 4. Unavailable historical Zopfli sample | **4A — leave as limitation** | Keep the five cells unavailable; do not invent or publish the comparison and do not require a new sample for this general release. A future new sample would be a separately identified measurement. |
| 5. Missing supplemental samples | **5C — required only for full admission** | The 20–50 real-toolchain PDFs and representative survey FITS sample do not block the general opt-in release, but remain required for full PDF/FITS admission. Existing corpora must not be relabelled as those samples. |
| 6. Evidence retention | **6B — durable artifact required** | A durable measurements/evidence Release artifact is required before publication. This selects the retention policy only: creating or publishing that Release still requires the owner's separate final confirmation immediately beforehand. |
| 7. F-06 evidence without a >2 GB file | **7A — sufficient for opt-in** | The signed-P standard/source evidence, arithmetic boundary test and ordinary CI are sufficient for the opt-in scope. No large-file integration run is required for this release; no full large-file stability claim is made. |

## Remaining release path

1. The coder lands the targeted F-11 convention/policy correction in PR #73. No additional product scope is
   authorised by these decisions.
2. Fresh CI and HEASARC checks and a targeted review of the new SHA are required. The expected red
   measurement comparison remains an open investigation, not something to make green by editing history.
3. The manager owns the F-09/F-12 documentation corrections and release narrative. The narrative must say
   opt-in implementation, retain the historical/current distinctions, disclose unavailable Zopfli evidence,
   and avoid full PDF/FITS admission or stability claims.
4. Before publication, prepare a durable evidence package tied to the exact final code/release identity,
   including the final CI/HEASARC/measurement records, run URLs, manifests and SHA-256 inventory. Artifact
   existence or an expired CI URL is not a substitute for this package.
5. The owner must separately authorise the actual merge, measurements/evidence Release upload, tag and
   public release. No issue is closed automatically.

## What remains open after the general release

- The investigation selected in 3A, the supplemental samples in 5C, and full PDF/FITS admission remain open.
- #57/#59 are not closed by the release-scope decision. #70 is an implementation tracker, not proof of full
  format admission; any closure is a separate owner action.
- F-09/F-12 documentation work and the O-3/O-4/O-5 dispositions must not be silently described as completed.
- F-04 remains retracted. No PDF/FLAC promotion-positioning work is added by these decisions.

## Execution update

The owner separately authorized the PR #73 merge on 2026-10-27. It completed as merge commit
`a404837b391337dbfbcda86e425e08960a65c782`; post-merge main CI and lockfile runs passed. This does not
constitute tag, Release-asset, public-release, issue-closure, or full PDF/FITS-admission approval. See
[POST-MERGE.md](../iteration-a4a0fdf/POST-MERGE.md).
