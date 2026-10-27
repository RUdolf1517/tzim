FITS review regression evidence (local, 2026-10-27)

Scope
- These records concern F-06/F-07/F-08, F-10/F-11 and the HEASARC
  controlled-refusal changes currently in the worktree.
- No O-3/O-4/O-5 change is included.
- The original three HEASARC controlled-refusal tests (unparseable candidate,
  unparseable funpack reconstruction, short descriptor) are generic robustness
  cases, not the three O-1 field paths. They are retained separately below.
- Three additional, field-specific O-1 regressions now cover PCOUNT, BITPIX and
  GCOUNT as requested. The malformed values used are PCOUNT=-1, BITPIX=0 and
  GCOUNT=0; the repository handoff available here names these fields but does
  not provide more precise mutation values.

1P descriptor bound
- FITS Standard v4.0, official NASA FITS Support Office PDF:
  https://fits.gsfc.nasa.gov/standard40/fits_standard40aa-le.pdf
- Sections 5.2.3 and 7.3.5 specify two 32-bit signed integers for a P array
  descriptor and say negative descriptor values have no defined meaning; the
  32-bit signed range ends at +2,147,483,647. Thus the accepted non-negative
  count and zero-based heap offset are bounded by exactly 2^31 - 1.
- The pinned HEASARC gate uses CFITSIO commit
  113781d8aa442cf8d3c27295d9084674b8b039f2 (archive SHA-256
  d93c0de372a264156d2f3e1eab2d4159e132c35c2216f71c0af0009b92a72b5e).
  In docs/cfitsio.tex it describes P descriptors as 32-bit and Q as 64-bit,
  and explicitly labels its unsigned-P interpretation as a local extension.
  utilities/fvrf_data.c warns when either P length or offset exceeds
  2147483647. The optional unsigned extension is not adopted; no 1Q contract
  expansion is made.

HEASARC controlled-refusal regression
- Regression suite: .github/scripts/test_fits_heasarc.py
- Baseline script: .github/scripts/fits-heasarc.py from commit
  9de2b7069a522ab5ce05e8255a3ce4c530f6190f, imported from a temporary copy.
- Baseline: 3 tests errored. The candidate parse and funpack reconstruction
  parse each escaped with ValueError: shorter than one 2880-byte record; the
  short-descriptor path exposed the missing safe_inflate_tiles guard (and the
  test independently confirms inflate_tiles raises struct.error).
- Current script: the original 3 generic tests passed, each recording a
  controlled refusal. The combined six-test suite (including the separate
  O-1 cases below) passed 6/6; its raw log is
  `heasarc-all-regressions-passing-after.log`.
- Generic before/after logs are retained as
  `heasarc-controlled-refusal-failing-before.log` and
  `heasarc-controlled-refusal-passing-after.log`; the HEASARC script's own
  self-check log is `heasarc-selfcheck-after.log`.
- These local runs use Python 3.11.2. The workflow adds the same unit-test
  command and retains its result in the 90-day HEASARC evidence artifact.

O-1 reviewer-handoff field regressions
- The reviewer-handoff fields named by the owner are tested independently:
  PCOUNT=-1, BITPIX=0, and GCOUNT=0 in the ZIMAGE BINTABLE header.
- Against the pre-change script, PCOUNT=-1 and GCOUNT=0 were accepted by the
  gate when fitsverify/funpack were mocked as successful; BITPIX=0 escaped as
  an uncaught ValueError while parsing the candidate. The mocks isolate the
  gate's own parser/acceptance path; the actual CI gate still uses real tools.
- After the parser validation, all three return a controlled refusal at
  `parse tzim candidate`, identifying the offending card and retaining the
  candidate bytes and JSON evidence. The three-case after run passed 3/3.
- Before/after logs are `o1-pcount-bitpix-gcount-failing-before.log` and
  `o1-pcount-bitpix-gcount-passing-after.log`. These are distinct from the
  earlier generic controlled-refusal logs and should not be conflated.

FITS oracle F-10/F-11 regression
- The before/after log uses the baseline oracle from commit 9de2b70 and the
  current oracle on a FITS file whose sample bytes are changed from +0 to -0
  and from one quiet-NaN payload to another.
- Baseline check_rewritten reported no problem; current check_rewritten reports
  a FITS-storage-byte difference at byte 0. Current self-checks also confirm
  that legal GZIP_1 for float data is convention-compliant, while it is not
  tzim's current writer policy, and that unsupported BITPIX values are rejected.
- Environment: Python 3.11.2, NumPy 2.4.6, Astropy 8.0.2. Raw output is
  fits-oracle-bitwise-before-after.log. All 8 fixtures were generated; the
  float-specials FITS storage bytes round-tripped through Astropy exactly
  (16,384 bytes; SHA-256 00e43781570a6a0c0c47755c27cd37fcf4cc86e22dbf4d85d2a9dc1c625cf3a9).

Validation limits
- At capture time, Python syntax checks, the HEASARC unit tests/self-check, the
  oracle self-check, workflow YAML parsing, bash -n for its bash steps, and
  git diff --check passed locally.
- This folder records local pre-push evidence only. cargo, rustc, rustfmt,
  fitsverify, fpack and funpack were not installed in the local workspace, so
  this record does not claim a local Rust build or actual-binary gate result.
  Remote CI and HEASARC run artifacts for the validation commit are the source
  for those results.
