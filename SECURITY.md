<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Security Policy

## Scope

All crates in this workspace (`mcp-conformance-core`, `mcp-trace-validator`,
`mcp-everything-server`, `mcp-reference-host`), the trace corpus, and the CI
configuration. The project's threat model lives in
[docs/plan/05-security-model.md](docs/plan/05-security-model.md).

## Supported versions

Until 1.0, fixes land on the latest 0.x minor only. This table is updated at
every release.

| Version | Supported |
| ------- | --------- |
| 0.2.x   | yes       |
| 0.1.x   | no        |

## Reporting a vulnerability

**Do not open a public issue.** Report privately via GitHub draft security advisories:

<https://github.com/tomtom215/mcp-conformance/security/advisories/new>

Include: a description and impact assessment, reproduction steps or a proof of
concept (a minimal trace file, if relevant), affected crate(s), and a suggested fix if
you have one.

## What to expect

- **Acknowledgment within 3 business days.**
- **90-day coordinated disclosure**, negotiable with the reporter; we aim to ship
  fixes well inside it.
- **P0 calibration**: vulnerabilities with CVSS ≥ 7.0 (the MCP ecosystem's own P0
  definition) target resolution within 7 days.
- **Advisories are published twice**: as a GitHub Security Advisory *and* filed with
  [RustSec](https://rustsec.org) for any affected published crate — `cargo audit`
  users must never be the last to know.
- Reporters are credited in the advisory and release notes unless anonymity is
  requested.

## A note on the everything server

`mcp-everything-server` is a *test fixture by design* — it deliberately exercises
every protocol capability. Its transport defaults are loopback-only with `Host`/
`Origin` validation enforced, and weakening them requires an API named
`dangerously_…`. Reports that it is "too permissive" *behind* those defaults are
working as intended; reports that the defaults can be bypassed are exactly what this
policy is for.
