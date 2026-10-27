# Manager acceptance review — PR #73 at `4a90ade`

Date: 2026-10-26. Review target: `4a90ade5770da10ace7f3bdde2d6215975a11a7a`.

## Evidence boundary

The manager inspected exact Git source and diffs, GitHub run/job/check status, PR comments, artifact metadata, and the retained full-SHA manifest and per-file measurement rows. The manager did not run the Rust binary, HEASARC tools, Python test suite, or a fresh corpus measurement. Final Actions ZIP contents remain uninspected because downloads are unavailable in this environment.

A bounded arithmetic replay of the new comparison functions was performed against the retained JSONL and the **full-SHA** source manifest. This checked comparison behaviour; it was not a new measurement and is not presented as the final CI artifact.

Verified at `4a90ade`: CI `40375638842` passes, HEASARC `40375638830` passes, and measurement `40375639073` is red. The measurement artifact `12674623068` and HEASARC artifact `12674782250` exist and are unexpired. The measurement job's comparison step fails, its report-publication step also fails, and artifact upload succeeds.

## #71 — accepted

The two previous manager findings are closed in source and by the actual green gate:

1. Normal rewritten candidates and the corrupt-tile control now enter the same `check_candidate` helper. The intact candidate must already have passed with no problems. The corrupted candidate must be rejected by that helper, and `payload_rejected` additionally requires a nonzero `funpack` result or an array mismatch. It is no longer enough for the separate `inflate_tiles` helper to notice a changed bit.
2. The pinned external Astropy `double_ext.fits` identity matches the retained full-SHA manifest exactly: full Astropy commit `dbbf2acd6dbb177db184bff319d2a2950dd178bb`, archive SHA-256 `3c6b762797ea8f292d5c35fbb3ebe82b7a28139783b5c6e870a38e576d62a46f`, selected file SHA-256 `7ab105b7695dbebd7da0bf430f58527b0da8223ea990460d22593c543f757bfc`, 11,520 bytes. The real RICE_1 input must remain byte-identical with two `EXTNAMX` cards. Each literal external card also enters a separate synthetic uncompressed rewrite case and the complete shared `fitsverify`/`funpack`/array/literal-card path. This avoids a duplicate-keyword warning without introducing a warning allowlist.

The inspected intermediate PR report `6423614362` is useful fail-before evidence for the final external-fixture adjustment: the combined duplicate-card rewrite preserved both cards but was rejected by `fitsverify`; the same report records the shared corrupt-tile control detected with `fitsverify=0` and nonzero `funpack`. The final diff changes the external cases to one card per rewrite, and the exact final gate is green. The intermediate report is not relabelled as the final run.

The scoped HDRLET preservation case remains accepted as previously documented. Artifact contents are not claimed to have been read; acceptance is based on the exact source, actual run status, pinned identity, and inspected PR evidence.

## #74 — accepted for the assigned scope

The Rust writer and Rust test trees are unchanged from the already reviewed `04b303e7`; the new commit changes only the Python harnesses and their workflows. The earlier actual-binary fail-before/pass-after evidence therefore still applies. Together with the accepted #71 gate, this closes the assigned F-01/F-02/F-03/F-05 and N-1/N-2/N-3 work. F-04 remains retracted. F-06 through F-12 remain open and are not closed by this verdict.

## #72 — comparison logic accepted; issue held for a concrete report-publication defect

The new comparison code addresses the manager's substantive findings:

- Published floating-point cells use explicit per-cell precision with decimal half-up rounding; an undeclared float precision is invalid rather than silently tolerated.
- FITS Tier 1 and Tier 2, qpdf mean/max, status/refusal maps, exact-failure identities, and input repository/commit-prefix/file/byte rows are compared.
- The historical 150-file Zopfli sample has five explicit `unavailable` rows and adds a problem; it is not silently omitted or replaced with an invented sample.
- Input-corpus and result-table problems contribute to the overall nonzero exit. The red run is the correct outcome, not something to repair by changing the baseline.

The bounded replay against retained rows and the full-SHA manifest produced the same substantive result reported by the coder:

- FITS result-table cells reproduce; five input-byte cells differ: Astropy `780 → 675 KiB`, CFITSIO `60 → 678 KiB`, DrizzlePac `3.3 → 14.8 MiB`, reproject `2.0 → 1.8 MiB`, and total `17.8 → 17.9 MiB`. Repository, full-commit/prefix, and file-count checks pass; PDF input-table cells reproduce.
- Four PDF result-table cells differ: faithful mean `0.39 → 0.42` at printed precision, positive share `68 → 69`, qpdf mean `1.68 → 1.65`, and render pages `173 → 168`.
- Five Zopfli side-table cells are explicitly unavailable. The render path-order audit passes with digest `c29f948d20ab97bf401c0b8dcf0e6bf64ea15bc8e27060e43c94a420443b412b`.

### Remaining defect: the full report is not published

In `.github/workflows/measure-repro.yml` line 161, the `ci-report.sh` invocation ends in **two backslashes** before the newline. The first escapes the second, so the newline is not continued: a literal `\` becomes the first script argument, the required label is absent, and the following line is a separate command. This is consistent with the observed failed `Publish failure log` step and absence of a measurement comment for `4a90ade`. Parsing YAML or Python cannot catch this shell-argument error.

Put the invocation on one physical line or use exactly one trailing backslash. Then allow the PR-triggered run to execute and provide the generated complete report. The measurement step should remain red for the real discrepancies/unavailable sample; the publication step should succeed, and the full table should be readable without downloading the ZIP.

#72 is otherwise substantively complete for the requested runner/comparison work, but an implemented report relay is not a passed report relay. Acceptance waits for that exact corrected run. Historical reproduction and owner disposition of the exposed discrepancies are separate decisions.

## Release disposition

- #71: **accepted** at `4a90ade`.
- #74: **accepted for its assigned scope**; no broader format certification.
- #72: **held only for the broken full-report publication and its corrected run**; comparison logic and red discrepancy handling are accepted in source/replay.
- FITS release: **still blocked**. #72 is not yet accepted, F-06…F-12 remain open, and the missing survey-FITS/source-toolchain-PDF samples and historical mismatches still require owner decisions. No issue closure, documentation rewrite, measurements Release, or publication is authorized by this review.
