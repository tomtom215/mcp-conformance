<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Conformance results

A conformance toolkit should be willing to measure real implementations — its own
and others' — against the official suite, and publish what it finds. These
measurements are stewardship artifacts: every number is *the official suite's*
number, captured by running it, with this project's registry used only to read
each failure back to a specific spec clause. Every claim has a command that
reproduces it.

## This project's reference implementations

The reference server and host are a **hard CI gate**, not a one-off measurement:
on every run, the [conformance job](https://github.com/tomtom215/mcp-conformance/blob/main/.github/workflows/ci.yml)
drives the pinned official suite against `mcp-everything-server` (server
scenarios) and `mcp-reference-host` (client scenarios), and replays the captured
sessions through the validator for the agreement check. A regression fails the
build. The live status is whatever the latest `main` run reports.

## rmcp tier-gap report

The published measurement of the official Rust SDK
([`rmcp`](https://github.com/modelcontextprotocol/rust-sdk)) against the suite is
the worked example of the method:

> 📄 **[rmcp tier-gap report — `2025-11-25`](https://github.com/tomtom215/mcp-conformance/blob/main/docs/reports/rmcp-tier-gap-2025-11-25.md)**

It records the server-scenario pass count, the requirement-level reading of each
failure, a concrete close-the-gap checklist, and the exact commands to reproduce
it — including a note on why the suite's `tier-check` aggregate is *not* the
figure to trust (it is GitHub-token-gated and its conformance counter carries a
known upstream counting bug). The headline finding at the report's measurement
date: two failing server scenarios, **neither of which is a `2025-11-25`
normative-clause violation** — one sits just below the spec's RFC 2119 floor
(template-argument substitution is schema prose), the other is a SEP-1330
serialization bug already filed upstream. That is the kind of distinction a
requirement-level tool exists to draw; see the report for the current count and
the close-the-gap steps.
