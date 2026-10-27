# Response to coder — `4a90ade`

## Verdicts

- **#71 accepted.** The shared `check_candidate` path closes the corrupt-tile finding. The intact candidate must pass first; corruption must then be rejected through `funpack` or array comparison. The pinned Astropy identity matches the retained full-SHA manifest. Both literal `EXTNAMX` cards separately exercise the complete rewrite path, while the real RICE_1 file is checked as an unchanged skip; no warning allowlist was introduced. HEASARC run `40375638830` is green.
- **#74 accepted for its assigned scope.** Rust writer/tests are unchanged from reviewed `04b303e7`, and #71 is now accepted. F-04 remains retracted; F-06…F-12 remain open.
- **#72 comparison logic is accepted, but the issue remains held for one concrete workflow failure.** Per-cell precision, complete table/input comparisons, explicit Zopfli unavailability, and red-on-discrepancy behaviour are correct in source and bounded replay. Do not change the baseline to make this run green.

## Required fix before #72 acceptance

`.github/workflows/measure-repro.yml` line 161 ends with **two backslashes**:

```yaml
          bash .github/scripts/ci-report.sh \\
            "${RUNNER_TEMP}/measure-repro-report.md" measure-repro failure full
```

In shell, the first backslash escapes the second. The newline is therefore not continued: a literal `\` becomes `$1`, the required label `$2` is missing, and the next line is a separate command. This explains the failed `Publish failure log` step and absent measurement comment at `4a90ade`; it is not evidence of a token permission problem.

Put the invocation on one physical line (preferred) or use exactly one trailing backslash. Let the PR-triggered workflow run. Expected outcome:

1. measurement remains red for the real differences/unavailable historical sample;
2. `Publish failure log` succeeds;
3. the complete report is readable in the PR, including the four PDF table differences, five FITS input-byte differences, and five unavailable Zopfli cells;
4. the evidence artifact still uploads.

A manager replay of the new comparison functions against retained rows and the full-SHA manifest already confirmed those comparison outcomes. This was not a fresh measurement or a substitute for the corrected CI report.

FITS release remains blocked by #72, F-06…F-12, missing supplemental samples, and owner decisions. No issue closure, product-documentation edit, measurements Release, or external publication is authorized.
