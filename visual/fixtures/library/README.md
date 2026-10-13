<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# The workbench's fixture library

The library `mcp-conformance serve` shows in the screenshot suite, and that the
workbench's Rust tests copy (`crates/mcp-conformance-cli/src/workbench/fixture.rs`).
Each trace's sidecar fixes its tags and recording date, so every view renders the same
on every machine; the verdict cache is computed when a copy is opened, never committed.

| Trace | From |
|-------|------|
| `everything-stdio`, `everything-http` | `corpus/good/stdio-feature-session.jsonl`, `corpus/good/http-session.jsonl` |
| `sdk-{go,python,rust,typescript}-2025-11-25` | `corpus/captured/` |
| `sdk-rust-2026-07-28` | `corpus/draft/captured/` |
| `base-001-request-id-boolean`, `tool-005-name-length` | `corpus/violations/` |
| `hostile-markup` | written for the workbench: markup in names, ids and payloads, an escape character in a method and a bidirectional override in a tool name — rendered, it must stay text |
| `leaky-session` | written for the workbench: a trace as a recorder without a header allowlist would write it, with an `Authorization` header, a cookie, a tool argument and a `_meta` value — each `fixture-…`, obviously not a credential — that redaction must remove |

A trace copied from the corpus changes only if the corpus copy does; refresh it with
`cp` and review the screenshot diff.
