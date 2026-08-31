# Security policy

## Supported versions

Rylus is pre-release software. Security fixes are made on the default branch and included in the
next release; older snapshots are not maintained as separate security branches.

## Reporting a vulnerability

Please report security issues privately through this repository's
[private vulnerability reporting](../../security/advisories/new) flow. Do not open a public issue
for an undisclosed vulnerability.

Include the affected revision and platform, impact, and a minimal reproducer when practical. You
should receive an acknowledgement within seven days and a triage decision within fourteen days.

## Deployment boundary

Rylus exposes screen capture and input control over the network. Keep authentication and TLS
enabled, bind only to networks you trust, and do not expose the service directly to the public
internet. The threat model and completed review are documented in
[docs/SECURITY-REVIEW.md](docs/SECURITY-REVIEW.md).
