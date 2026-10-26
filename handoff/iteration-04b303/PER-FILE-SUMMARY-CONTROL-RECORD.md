# Per-file and control record — FITS/PDF follow-up

## Evidence identity and limits

- **Tested source SHA:** `04b303e7f8a410f8b71e86b2517bccae61079036` on `arena/7ded2747-tzim`.
- **CI:** [40359915275 — `ci`, success](https://github.com/RUdolf1517/tzim/actions/runs/40359915275).
- **FITS gate:** [40359915228 — `fits-heasarc`, success](https://github.com/RUdolf1517/tzim/actions/runs/40359915228).
- **Measurement replay:** [40359915023 — `measure-repro`, completed with the PDF publication comparison red](https://github.com/RUdolf1517/tzim/actions/runs/40359915023); [readable run summary](https://github.com/RUdolf1517/tzim/pull/73#issuecomment-6421444834).

The final Actions artifact and job-log downloads returned `EOF` in this environment. The tables below state the **assertion-level results implied by the successful gate** and the expected fixture contract. They do not invent candidate byte counts, candidate hashes, or exact per-file tool output that was only in the inaccessible artifact. Fixture input sizes and SHA-256 values were regenerated from the committed harness's deterministic fixture builder; they are input identities, not output hashes. A zero-exit FITS gate means every fixture and all three controls below passed their implemented checks.

## FITS fixtures

| Input fixture | Input bytes | Input SHA-256 | Contract result | Passing assertions |
|---|---:|---|---|---|
| `int16_primary.fits` | 66,240 | `6f06852a294e110f6e7e10dc7a825ab8a80ce03cc24c2b92900b29c6e45b4ed7` | rewritten | One primary image; new empty primary plus compressed image table; `funpack -C` array round-trip and literal card diff pass. |
| `float32_specials.fits` | 37,440 | `08d3feceecf29aa5e166fa8746a38f9d1c826c07119430f069a9e3dc66fc46ae` | rewritten | Float image round-trip is source vs `tzim → funpack -C`, byte-for-byte, including both signed zeros and the distinct NaN payload words; literal card diff passes. No `fpack` float reference is used. |
| `float64_extension.fits` | 43,200 | `fec2778d457e7e6251594fef557e6194d852b35589f6e50f76bf6a80436307c5` | rewritten | Image extension is compressed without changing the HDU count. The harness parses the quoted `XTENSION` value up to its closing quote, before the comment. |
| `multi_hdu.fits` | 37,440 | `1c7cf5585f9f8917b4eee607347d6c2c145b3e01aa75a7e12e085a1d46073aec` | mixed | Two registered image HDUs are rewritten; the registered BINTABLE is copied verbatim. Original and candidate `fitsverify` must both be clean; `funpack -C` arrays and literal cards pass. |
| `hdrlet_preservation.fits` | 282,240 | `9b06ae21428ff277dab2bff7425d259afc03aa94f3e385a2f2736c69754f58af` | mixed | One image is rewritten; HDRLET and BINTABLE bytes are preserved verbatim. The separate HDRLET case compares candidate diagnostics to this input's exact per-file baseline and rejects new markers/counts. It does not use a global warning allowlist. |
| `tiny.fits` | 5,760 | `9e137e3c2f943710cb690900808d308166fbbde41c6fe8be69e9be8436a4a670` | unchanged + `already_optimal` | Candidate is not accepted as a rewrite; input remains byte-identical. |
| `scaled.fits` | 8,640 | `7ee96c4174c1dbf28f8ac21b11b311cf1dfbe96cd16e4c78602a3b521bb3a595` | unchanged + `skipped` | Non-identity `BSCALE`/`BZERO` is refused; input remains byte-identical. |
| `checksummed.fits` | 20,160 | `770d2322b396402549956b9d055bcbee045b15e7438bbbe870426ae494f85347` | unchanged + `skipped` | CHECKSUM/DATASUM input is refused rather than rewritten with stale checksums. |
| `rice.fits` | 8,640 | `41a01b25e02c44938f0a4bf93e1cc2b113a64c27e661df29de4ab49b7e703fa1` | unchanged + `skipped` | Existing RICE_1 compressed image is not rewritten by this build. |
| `zerowidth.fits` | 2,880 | `c1315f34679e6be88967e69a0123c134dc45b1f516306ebd48b38b85b2f307bc` | unchanged + `skipped` | Zero-length image axis is outside the accepted subset; input remains byte-identical. |
| `double_ext.fits` | 37,440 | `49bf09155355b99d41664266a5f696db92358c3674f35ae841dfed806e16c185` | rewritten | All three eligible image HDUs are rewritten; primary adds one HDU; `funpack -C` array and literal-card assertions pass. |
| `zero_width_column.fits` | 17,280 | `57eb1edaadd40271e5e4a75e07c73d38008854fd235ae789fad1b1db4b58b3cf` | unchanged + `already_optimal` | The whole file remains byte-identical; its BINTABLE is preserved. No discarded-candidate size is used as an external gate criterion. |

### Controls

| Control | Result on successful gate | Meaning / limit |
|---|---|---|
| Corrupt one bit in a candidate tile | **Pass:** intact inflated tiles match the source; after the flip, they differ or fail to inflate, so the integrity comparison detects corruption. | `fitsverify` accepting structurally valid but corrupted image data is not itself a failure of this negative control. Its exact exit code is not asserted here. |
| Break table structure (`NAXIS1=16`, `TFORM1` still says 8) | **Pass:** `fitsverify` returns nonzero. | Proves the structural negative control is detected. |
| Positive toolchain control: `fpack → funpack -C` | **Pass:** `fpack`, `fitsverify`, and `funpack -C` succeed; literal 80-byte-card difference count is zero; integer arrays are equal. | This control is integer-only and does not serve as a floating-point reference. |

## Passing-after Rust test coverage

The successful `ci` run compiled and ran the tests on Linux, macOS, and Windows, in default and all-features configurations. Relevant tests include:

- `fits_random_groups_use_the_legal_data_length_and_are_skipped` — legal `NAXIS1=0`, `PCOUNT=17`, `GCOUNT=3` data length; a misleading header embedded in payload is not traversed as an HDU; file remains unchanged.
- `fits_simple_false_is_refused_without_rewriting` — `SIMPLE=F` refusal.
- `fits_structural_cards_and_comments_are_encoded_verbatim_in_z_forms` — full Z-form card mapping, including `ZEXTEND`.
- `fits_decimal_and_exponent_identity_scaling_is_accepted_exactly` and `fits_range_sensitive_non_identity_scaling_is_refused_exactly` — decimal/exponent handling without f64-only lossy transforms.
- `fits_structural_cards_and_comments_are_encoded_verbatim_in_z_forms`, `fits_every_other_hdu_is_copied_verbatim`, and the positive registered `multi_hdu.fits` gate — traversal and preservation checks.

The current FITS gate pass is on `04b303e…`; the retained earlier fail-before record is [`i71-fail-before-run-40350665261.md`](../i71-fail-before-run-40350665261.md). The previous failed HEASARC run is not pass-after evidence.

## Measurement/provenance record

- The seven full-SHA corpora contain **2,386 selected files / 190,630,759 selected bytes**. Their selected-file manifest digest remains `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`; archive-byte changes are recorded as new baselines, not described as archive equality. The full-SHA comparison and manifest are [`fullsha-repin-comparison.md`](../fullsha-repin-comparison.md) and [`fullsha-source-manifest.json`](../fullsha-source-manifest.json).
- The final `measure-repro` run verified the corpus manifest with `0 problem(s)` and reported **`fits: reproduced`**. It reported PDF `share_positive=69.37561942517344` vs published `68`, and `render_pages=168` vs published `173`; the run's bundle digest is `2739270dcb3f6e4500032ee160e4c8c27a5ef80f5a38675308836c38d2d3b2a7` (12 evidence files).
- The exact saved 140 render paths, sorted accepted-path order, and Python 3.11/3.12 comparison are in [`pdf-order-runtime-audit.md`](../pdf-order-runtime-audit.md) and [`i74-metric-source-records/render-sample.jsonl`](../i74-metric-source-records/render-sample.jsonl). NUL-delimited ordered-path SHA-256: `c29f948d20ab97bf401c0b8dcf0e6bf64ea15bc8e27060e43c94a420443b412b`.
- The zlib comparison covers **6,344 streams** across zlib 1.2.13 and 1.3.1: no compressed-length differences and no `faithful_pct > 0` flips; **700/1,009** positive PDFs in each environment. Per-file records are in [`zlib-borderline-classification.json`](../zlib-borderline-classification.json).

The saved `i74-metric-source-records/fits.jsonl` and `pdf.jsonl` are source rows from the earlier #72 replay; they are **not** mislabeled as per-file output from the final run. The final run's per-file measurement JSON is in the inaccessible Actions artifact. This record therefore does not prove its per-file output hashes, bytes, or candidate row results. The old 68% and 173-page values remain unresolved; neither was changed or declared wrong.
