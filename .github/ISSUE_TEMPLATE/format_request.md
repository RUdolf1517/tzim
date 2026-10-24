---
name: Format request
about: Propose support for a file format
title: 'format: '
labels: format, research
assignees: ''
---

Before opening this issue, read
[docs/formats.md](https://github.com/RUdolf1517/tzim/blob/main/docs/formats.md#4-admission-criteria-for-a-new-format)
and check that the format is not already listed as refused.

## Format

- Name and common extensions:
- Typical files per user, and typical sizes:
- How is the format detected from its content (magic bytes, structure)?

## Lossless optimization

- What exactly can be made smaller without losing information?
- What is the expected saving? Numbers with a source beat estimates.
- Does the optimization preserve every byte except the compressed payload, or does it require normalisation
  of any structure? List everything that would change.

## Guarantee and verification

- Which guarantee class applies: format-equivalent, or exactly reversible?
- What would an independent verification do, and which existing decoder could it use?
- What can go wrong that the verification would miss?

## Safety

- Are there signatures, checksums over whole-file bytes, alignment requirements, or "must stay stored"
  constraints that a rewrite would invalidate?
- Which inputs must be refused outright?

## Prior art

- Existing tools that do this (with links):
- Known bugs or data-loss reports in those tools:

## Sample files

<!-- Small samples or links. Do not attach confidential material. -->
