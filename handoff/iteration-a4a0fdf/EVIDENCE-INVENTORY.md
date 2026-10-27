# Durable evidence inventory — merged code `a404837b`

Prepared: 2026-10-27; updated after the authorized PR merge. This is a **local, pre-publication inventory**
for merged code SHA `a404837b391337dbfbcda86e425e08960a65c782` (PR head
`a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`). It is not a GitHub Release, an uploaded asset, a tag, or full
PDF/FITS acceptance. If the release code or documentation changes, this inventory must be regenerated for the
new final SHA rather than silently reused.

## 1. Source and merge identity

| Item | Value |
| --- | --- |
| Repository | `RUdolf1517/tzim` |
| Reviewed PR head SHA | `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8` |
| Merged code SHA on `main` | `a404837b391337dbfbcda86e425e08960a65c782` |
| Merge completed | 2026-10-27T14:49:33Z, PR #73, explicitly authorized by the owner |
| Merge commit parents | baseline `5dd45b38412ffb762a1fc380bd291351c9e6601e`; PR head `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8` |
| Head and merged tree | `80e61c2830c4a6ac52bb590b046e06deadbc8a25` (identical, so the source hashes below apply to both) |
| PR head branch reported by Actions | `arena/7ded2747-tzim` |
| Baseline parent | `5dd45b38412ffb762a1fc380bd291351c9e6601e` |
| PR test merge SHA | `dcc7e35c75cd7b6705771c617be2f984cf032311` |
| Merge parents verified through GitHub | baseline `5dd45b38412ffb762a1fc380bd291351c9e6601e`; head `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8` |
| Targeted code delta | `fba375915aaade7c9d87babe4964a846b4a51380..a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`; only `.github/scripts/fits-oracle.py` |

The following are SHA-256 hashes of the exact Git blobs at the reviewed PR head, calculated locally from
`git show`. The merged commit has the identical tree, so these hashes also identify the blobs at merged SHA
`a404837b391337dbfbcda86e425e08960a65c782`:

```
58f9bd0c1de06e1667bd8d6f07c5b6fe9ad01969c34eeb68334838cd59a1d91f  .github/scripts/fits-oracle.py
12d1dc32af06f4c235b55b5fa767cb9b5ec753ecbe3d741f3205ccbf98bb7910  .github/scripts/fits-heasarc.py
12e47aa154889de58f0f4660806d582dc98e408610a06a5ad9433f21c81de1fd  .github/workflows/ci.yml
5ad5a1d83380af845b4e2661edb9459090f55c1e2b73548c4db400ea171b0560  .github/workflows/fits-heasarc.yml
8f0f5434e3fd170928caabd29fac9899626c6575489c61746c75f95ea71c3a61  .github/workflows/measure-repro.yml
```

## 2. Fresh run inventory

Every row below was read from the GitHub run API with `head_sha=a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`.

| Workflow | Run ID | Event | Conclusion | Run URL |
| --- | ---: | --- | --- | --- |
| CI | 40522087890 | push | success | https://github.com/RUdolf1517/tzim/actions/runs/40522087890 |
| CI | 40522097091 | pull_request | success | https://github.com/RUdolf1517/tzim/actions/runs/40522097091 |
| fits-heasarc | 40522097164 | pull_request | success | https://github.com/RUdolf1517/tzim/actions/runs/40522097164 |
| fuzz | 40522097120 | pull_request | success | https://github.com/RUdolf1517/tzim/actions/runs/40522097120 |
| corpus | 40522097184 | pull_request | success | https://github.com/RUdolf1517/tzim/actions/runs/40522097184 |
| third-party | 40522097189 | pull_request | success | https://github.com/RUdolf1517/tzim/actions/runs/40522097189 |
| lockfile | 40522087463 | push | success | https://github.com/RUdolf1517/tzim/actions/runs/40522087463 |
| measure-repro | 40522098603 | pull_request | **failure, expected open comparison** | https://github.com/RUdolf1517/tzim/actions/runs/40522098603 |

Post-merge runs on `main`, both with `head_sha=a404837b391337dbfbcda86e425e08960a65c782`:

| Workflow | Run ID | Event | Conclusion | Run URL |
| --- | ---: | --- | --- | --- |
| CI | 40524876729 | push | success | https://github.com/RUdolf1517/tzim/actions/runs/40524876729 |
| lockfile | 40524876718 | push | success | https://github.com/RUdolf1517/tzim/actions/runs/40524876718 |

The HEASARC run is workflow ID `397069588`, run number 28. Its successful steps include tool-provenance
recording, controlled-refusal regression tests, pinned archive SHA-256 verification, building
`fitsverify`/`fpack`/`funpack`, external fixture verification, the FITS-enabled tzim build, the HEASARC gate,
evidence sealing, and evidence retention. The measurement run is workflow ID `397069590`, run number 28; its
comparison step failed and its failure-log/evidence steps succeeded.

The final PR check rollup contained **41 SUCCESS and 1 FAILURE** (the measurement comparison), with no queued
checks. Green CI is not writer acceptance or release approval.

## 3. Measurement report and source records

- Full report: PR comment [6444798874](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6444798874),
  authored by `github-actions[bot]`, created `2026-10-27T14:35:25Z`.
- Report marker: `tzim-ci:measure-repro:dcc7e35`; the merge parents are recorded above.
- Report body: 15,560 Unicode characters / 15,590 UTF-8 bytes; locally calculated SHA-256
  `6cfa65814ce749e034fea39f3616e620338ab8123d0f7958af608cb7a4a5352f`. A second single-comment API retrieval
  matched the saved body byte-for-byte and produced the same hash.
- New manifest digest printed in the report:
  `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5` (2,386 files).
- Render sample path-sequence SHA-256 printed in the report:
  `c29f948d20ab97bf401c0b8dcf0e6bf64ea15bc8e27060e43c94a420443b412b`.
- Open comparison rows: five FITS input-byte `DIFFERENT`, four PDF result-table `DIFFERENT`, and five
  unavailable Zopfli cells. FITS result-table medians are reproduced. No baseline or threshold was edited.

The complete report body retrieved for this inventory is held outside Git at
`/tmp/tzim-comment-6444798874.md` during this session. That temporary file is not itself durable retention;
the durable package must include a retained copy or an equivalent accessible record.

## 4. Artifact metadata (not content verification)

The following metadata came from the GitHub artifacts API. The `digest` values are **API-reported** artifact
digests, not hashes independently recomputed from downloaded ZIPs.

| Artifact | ID | Run | Bytes | API digest | Created | Expires | Stable metadata URL |
| --- | ---: | ---: | ---: | --- | --- | --- | --- |
| `fits-heasarc-evidence` | 12742092887 | 40522097164 | 255,820 | `sha256:bb9e30fd302b0c75f659a24436dafc2e810f214b97e53e7d248b3906907408b6` | 2026-10-27T14:34:01Z | 2027-01-25T14:32:34Z | https://api.github.com/repos/RUdolf1517/tzim/actions/artifacts/12742092887 |
| `measure-repro-evidence` | 12741408306 | 40522098603 | 413,344 | `sha256:b4e0b08829ec4d77e6baa65d256f4346671b3529d48c1893078d275ac55f0ae3` | 2026-10-27T14:35:27Z | 2027-01-25T14:32:34Z | https://api.github.com/repos/RUdolf1517/tzim/actions/artifacts/12741408306 |

Both artifacts were unexpired when read. One download attempt for each ended in `EOF` from the artifact-storage
redirect; no ZIP member list, raw log, or internal checksum was independently inspected. Do not convert the
metadata above into a claim that the artifact contents passed review.

## 5. Owner scope and publication hold

The owner selected **1A 2C 3A 4A 5C 6B 7A** on 2026-10-27. The complete record is
[OWNER-DECISIONS.md](../iteration-fba3759/OWNER-DECISIONS.md). In particular, historical FITS results are not
release/performance evidence, current differences remain under investigation before corresponding full
acceptance, Zopfli remains an explicit limitation, supplemental samples gate full admission only, and signed-P
evidence is sufficient for the opt-in scope.

Before any publication, the durable package must be refreshed to the exact final release SHA and include:

1. the final source/commit manifest and independently calculated SHA-256 inventory;
2. final CI, HEASARC, fuzz/corpus and measurement run identities, URLs, conclusions and relevant job/step
   records;
3. durable raw evidence or accessible retained copies, with downloaded-file hashes checked against the API
   metadata where possible, plus the complete measurement report and its corpus/source records;
4. the unresolved measurement rows and known limitations, without baseline/threshold edits; and
5. a record of the owner's separate confirmation immediately before creating or publishing the measurements
   /evidence Release asset.

**PR #73 was merged under the owner's explicit authorization on 2026-10-27. No tag, Release, issue closure,
external post, or package publication has been performed.** The merge contains the PR code/evidence; any later
manager-documentation or release-metadata commit requires a refreshed final source manifest and release SHA.
