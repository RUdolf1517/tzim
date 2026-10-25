# Audits

Independent audits that this project asked for, and the evidence behind them.

## What belongs here

An audit that produced findings, decisions or measurements the project is going to act on. A file here is
**not** documentation of the product — it is a record of what was true at one commit, what was checked, and
what was found. Product-facing conclusions belong in the documents the audit points at (`guarantees.md`,
`safety.md`, …) and in `CHANGELOG.md`.

## Rules

- **Name:** `YYYY-MM-DD-vX.Y.Z.md` — the date the audit was written and the version it is about.
- **Language:** English, like the rest of `docs/`. An audit is read by the same people who read the contract.
- **Frozen.** Once its findings are filed as issues, the file is not edited. When an issue is fixed, the fix,
  its test and its `CHANGELOG.md` entry are the record; the audit keeps saying what was true when it ran. A
  document that silently updates itself is a second source of truth.
- **State the baseline.** Every audit begins with the commit it was performed on and, when the branch moved
  during the audit, the commits it was re-verified against, plus every `file:line` reference pointing at the
  final one.
- **State the limits.** What could not be run, and therefore what is a claim about code rather than about
  behaviour. "Nothing was executed" is a finding about the audit, and it is written down.
- **Close it.** A finished audit carries `Status: closed` in its header and a map from each finding to its
  issue, so that the paper trail ends in something actionable.

## Not shipped

`Cargo.toml` sets `exclude = ["docs/audits/"]`: these files stay in the repository and out of the published
crate. They are history, and history does not need to be in the package.
