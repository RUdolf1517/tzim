# Security Policy

This document explains how to report a security vulnerability in Tzim. It does not replace the detailed
format contracts and file-replacement documentation linked below, and it is not a claim that the software is
free of vulnerabilities.

## Supported versions

Security reports are investigated against the **latest published release** and the current **`main` branch**.
Fixes are developed on `main` and distributed through a new release. Backports to older releases are not
guaranteed; users should upgrade to a release containing the fix.

Include the release version or source commit in your report so that the affected code can be identified
unambiguously.

## Report a vulnerability

**Do not disclose an unpatched vulnerability in a public issue, pull request, or discussion.**

Use either of these private channels:

1. **Preferred:** open the repository's [Security tab](https://github.com/RUdolf1517/tzim/security) and choose
   **Report a vulnerability** to submit a private vulnerability report through GitHub.
2. **Fallback:** email [rudolfzinovev@protonmail.com](mailto:rudolfzinovev@protonmail.com).

You do not need to send the same report through both channels. Do not include sensitive personal data or
private files unless they are essential to the investigation; a minimal, non-sensitive reproducer is
preferred.

## Scope

Reports about any default or opt-in Tzim feature are welcome. Security-relevant examples include:

- unintended modification, corruption, or loss of a file;
- bypass of candidate verification or a documented file-replacement safeguard;
- writes outside the paths selected by the user, arbitrary file overwrite, or unsafe symlink/hard-link
  handling;
- unexpected permission, ownership, or privilege-boundary violations;
- crafted input causing resource exhaustion disproportionate to the documented input limits;
- bypass of release-asset checksums or Windows installer integrity verification.

Ordinary format-compatibility bugs without a security impact should use the public bug-report template. A
documented limitation is not by itself a vulnerability, but a way to bypass a documented safeguard may be
one.

## What to include

Provide as much of the following as is available:

- the Tzim version or commit, operating system, architecture, and installation source;
- enabled feature flags and the exact command, including relevant options;
- what you expected, what happened, and the security impact;
- verbatim output or logs, with sensitive values removed;
- a minimal reproducer and, where useful, the format and provenance of the input file.

Do not run a suspected destructive reproducer against valuable data. Use disposable copies in a test
directory.

## Research and coordinated disclosure

Good-faith research that respects these boundaries is welcome. Test only files and systems you own or are
authorized to test, and do not access or modify third-party data, disrupt services, or use more exploitation
than is necessary to demonstrate the issue.

Reports are reviewed and coordinated with the reporter through validation, remediation, and disclosure.
Response and resolution times depend on the problem; this policy does not promise a fixed response or fix
SLA. We aim to keep the reporter informed and to agree on disclosure timing. Please keep vulnerability
details and proof-of-concept material private until that coordinated disclosure.

## Related documentation

- [Safe file replacement](docs/safety.md) — the replacement algorithm, atomicity, and failure modes.
- [Preservation guarantees](docs/guarantees.md) — format contracts and verification boundaries.
- [Testing and fuzzing](docs/testing.md) — test coverage and its stated limits.
- [Releases](docs/releases.md) — release artifacts, checksums, and installation channels.
