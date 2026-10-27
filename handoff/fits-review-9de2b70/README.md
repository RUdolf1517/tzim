# Handoff: independent FITS review of `9de2b7069a522ab5ce05e8255a3ce4c530f6190f`

This directory is a **handoff channel**, not a manager verdict. It carries the
reviewer's package to whoever checks the original records, so that the O-1…O-5
observations and the per-finding statuses can be audited against the runs
rather than against the prose.

| item | value |
| --- | --- |
| reviewed SHA | `9de2b7069a522ab5ce05e8255a3ce4c530f6190f` (PR #73 head, `OPEN`) |
| report | [`REVIEW-9de2b70.md`](REVIEW-9de2b70.md) |
| bundle | [`fits-verify-9de2b70-bundle.tar.gz`](fits-verify-9de2b70-bundle.tar.gz) |
| report sha256 | `9b607dc5b5f33d207f0a9fecc51362cd4345f3425ad56b25a1a0daf323f35a05` |
| bundle sha256 | `d4287857f14cf01af0acd10069096b7accc12d677684da372e96005fb3f85853` |
| bundle contents | `REVIEW-9de2b70.md`, `SHA256SUMS` (429 lines), `01-fixtures/`, `02-driver/`, `03-evidence/`, `04-tools/`, `05-extracted-at-sha/` |

## How to verify the package

```sh
sha256sum REVIEW-9de2b70.md fits-verify-9de2b70-bundle.tar.gz   # the two values above
mkdir /tmp/check && tar xzf fits-verify-9de2b70-bundle.tar.gz -C /tmp/check
cd /tmp/check && sha256sum -c SHA256SUMS                        # expect 429/429 OK
```

`03-evidence/` holds one `.cmd` / `.stdout` / `.stderr` / `.exit` file per run
(61 of them), plus `gate-acceptance/` with the gate's own artefacts,
`baseline.json` with the same numbers in machine-readable form, and three
auxiliary runs (`decimal-port-check.*`, `reference-writer-diff.*`,
`gate-crash-tracebacks.txt`).

## What is *not* in this package

There is no `tzim` binary, no `cargo test` output and no CI artifact. The
sandbox has no Rust toolchain (`cargo`/`rustc` absent, `static.rust-lang.org`
and `crates.io` unreachable) and `gh run download 40392951490 -n
fits-heasarc-evidence` failed with `EOF` three times. Everything that needs the
binary is therefore `not run`, and the report says so in those words.

## Statuses, for the record check

| finding | status | evidence class |
| --- | --- | --- |
| F-06 | `not run` | arithmetic bound only (v2.3 §3, `1P`→`1Q` above ~2.1 GB); no > 2 GB file was written |
| F-07 | `not run` | test inventory by reading (`fits.rs`: 12 tests, 5 `expect_err`, all on `optimize`; `verify()` called twice, both `.expect`) |
| F-08 | `not run` | 3 of 7 points closed in code; residual shown at the **gate** level (ZVAL1 mutation passes, §4.7) |
| F-10 | **`confirmed`** | produced by running the oracle at this SHA: NaN-payload and `-0.0→+0.0` both give 0 problems; the gate sees both |
| F-11 | **`confirmed`** | running the oracle: legal `GZIP_1` float → `GZIP_1 for ZBITPIX -32`; no oracle self-check exists in the code |

Three evidence classes are kept apart throughout and must stay apart when this
is read:

1. **the binary** — never built, never run (`not run`);
2. **layout transcription** — `02-driver/synthetic_layout.py` writes the file
   `write_tiled` describes and the real gate/fitsverify judge it (§4.4);
   this says the *described layout* is accepted, not that the code emits it;
3. **source reading** — the diff review in §6, the F-07 inventory, O-3/O-5.

## New observations, none of them accepted defects

O-1…O-5 in §7 are observations from this run, not adjudicated findings. In
particular:

* **O-1** is real and reproducible: 3 of 26 mutated candidates make the gate
  raise `ValueError` out of its own `parse` instead of returning a verdict
  (`03-evidence/gate-crash-tracebacks.txt`). The job still fails closed.
* **O-3/O-4/O-5** are triage notes. O-4 is explicitly limited to the recorded
  `fpack`/`imcopy` invocations in this environment (P-13 wording); nothing in it
  is a statement about `tzim`.

## Scope

FITS release is **not** approved by this package. F-04 stays retracted. The
earlier package under `handoff/fits-review/` is untouched, and PR #73 was not
modified: this handoff lands on `arena/cf9e3b68-tzim`.
