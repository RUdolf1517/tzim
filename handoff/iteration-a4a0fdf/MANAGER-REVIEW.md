# Targeted manager review — PR #73 at `a4a0fdf`

Date: 2026-10-27. Review target: `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`.

## Verdict

**The narrow F-11 correction is accepted for this review scope.** The registered-codec convention check,
integer `GZIP_2` handling, and strict tzim writer policy are now separated in the oracle, with self-checks.
This is not writer acceptance, full/stable FITS admission, a performance claim, issue closure, merge approval,
or release approval. PR #73 remained open at the time of this review; the subsequent authorized merge is
recorded in [POST-MERGE.md](POST-MERGE.md).

## Diff and source inspection

The only diff reviewed was `fba375915aaade7c9d87babe4964a846b4a51380..a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`.
It changes exactly one file, `.github/scripts/fits-oracle.py` (**79 insertions, 16 deletions**). No Rust writer,
`fits-heasarc.py`, `compare_arrays`, PDF/FLAC, measurement baseline, threshold, or product-code file changed in
this delta. `git diff --check` passed and the fetched Python source parsed successfully.

Source reading confirms:

- `GZIP_1`, `GZIP_2`, `RICE_1`, `PLIO_1`, and `HCOMPRESS_1` are convention-recognized separately from tzim
  policy; an unregistered codec remains a convention failure.
- The convention check no longer imposes a universal float-only rule on `GZIP_2`; integer `GZIP_2` passes the
  convention check while still failing tzim's policy, which requires `GZIP_1` for integer output.
- Registered codecs without decoder coverage have a distinct `not evaluated` result in the evaluation helper.
  The self-check exercises that path with a simulated reduced decoder set, checks the registered non-tzim
  codecs, integer `GZIP_2`, and an unknown codec, and retains the signed-zero/NaN-payload controls.
- The `check_rewritten` return-value change is matched by its only in-repository caller. No additional caller
  was found in the reviewed tree.

The new self-check is a function/control check, not a real-file corpus of RICE/PLIO/HCOMPRESS rewrites. No
broader codec support or independent decode coverage is inferred from it.

## Fresh execution evidence

All identities below name the reviewed HEAD, not an earlier package:

| Check | Run | Result at `a4a0fdf` |
| --- | --- | --- |
| Push CI | [40522087890](https://github.com/RUdolf1517/tzim/actions/runs/40522087890) | success |
| Pull-request CI | [40522097091](https://github.com/RUdolf1517/tzim/actions/runs/40522097091) | success |
| HEASARC gate | [40522097164](https://github.com/RUdolf1517/tzim/actions/runs/40522097164) | success |
| Fuzz smoke | [40522097120](https://github.com/RUdolf1517/tzim/actions/runs/40522097120) | success |
| Real-file corpus | [40522097184](https://github.com/RUdolf1517/tzim/actions/runs/40522097184) | success |
| Independent decoders | [40522097189](https://github.com/RUdolf1517/tzim/actions/runs/40522097189) | success |
| Lockfile | [40522087463](https://github.com/RUdolf1517/tzim/actions/runs/40522087463) | success |
| Measurement reproduction | [40522098603](https://github.com/RUdolf1517/tzim/actions/runs/40522098603) | **expected failure; open investigation** |

The PR check rollup contained **41 successful checks and one failed measurement comparison**, with no queued
checks at the final read. The HEASARC job's controlled-refusal tests, FITS-enabled build, actual gate, evidence
sealing, and artifact-retention steps all report success. This is remote execution evidence; the manager did
not run Rust or HEASARC tools locally.

## Measurement identity and open rows

The full measurement report is PR comment
[6444798874](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6444798874). Its marker names merge
`dcc7e35c75cd7b6705771c617be2f984cf032311`; GitHub reports that merge's parents as baseline
`5dd45b38412ffb762a1fc380bd291351c9e6601e` and reviewed HEAD `a4a0fdfd3fdc0bb712542176e2657fc2b9d180a8`.
Thus the report is tied to this candidate, not to a stale head.

It retains the same open results: five FITS input-byte `DIFFERENT` rows, four PDF result-table `DIFFERENT`
rows, and five unavailable Zopfli cells. FITS result-table medians are reproduced. No baseline or threshold was
changed. Under owner decisions 2C/3A/4A, these are investigation/limitation records, not a reason to rewrite
history or claim full PDF/FITS acceptance.

## Artifact and evidence boundary

The API reports unexpired evidence artifacts for this HEAD:

- `fits-heasarc-evidence`, artifact `12742092887`, 255,820 bytes, API digest
  `sha256:bb9e30fd302b0c75f659a24436dafc2e810f214b97e53e7d248b3906907408b6`;
- `measure-repro-evidence`, artifact `12741408306`, 413,344 bytes, API digest
  `sha256:b4e0b08829ec4d77e6baa65d256f4346671b3529d48c1893078d275ac55f0ae3`.

Both download attempts ended in `EOF` from the GitHub artifact storage redirect. The API metadata and digest
are recorded, but the ZIP contents and job logs were **not independently inspected**. Artifact existence is not
content verification. The durable inventory is in [EVIDENCE-INVENTORY.md](EVIDENCE-INVENTORY.md); it must be
reconciled to the eventual final release SHA before any publication.

## Remaining scope guards

Owner decisions **1A 2C 3A 4A 5C 6B 7A** are recorded in
[OWNER-DECISIONS.md](../iteration-fba3759/OWNER-DECISIONS.md). They permit the general opt-in release path but
do not close #41/#57/#59/#70 or make historical FITS results release evidence. The later merge was separately
authorized and is recorded in [POST-MERGE.md](POST-MERGE.md); no tag or Release approval follows from it.
O-3/O-4/O-5 remain read-only triage. F-09/F-12 documentation corrections are manager-owned and are kept
separate from this code acceptance. No PDF or FLAC work was added.
