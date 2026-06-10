<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Trace corpus

Fixtures for the golden-corpus tests (`crates/mcp-trace-validator/tests/golden.rs`):

- **`good/`** — sessions that must validate with verdict `pass`.
- **`violations/`** — single-issue sessions, each falsifying at least one check;
  named after the requirement whose check they exist to kill.
- **`golden/`** — the byte-pinned expected report for every trace. Regenerate only
  via `cargo xtask bless` and review the diff like code.

## Provenance ledger

Every trace's origin, in one reviewable place that survives history rewrites (the
invariant test in `golden.rs` fails if a trace is added without a ledger row). All
current traces share one provenance: **hand-authored for this repository as
synthetic sessions** (no third-party traffic, no recorded production data),
written against the `2025-11-25` spec text fetched live from
modelcontextprotocol.io on 2026-06-09 and validated against the embedded registry
at the commit that introduced them. Traces produced by capture tooling (roadmap
M3) will record the capturing implementation and revision here.

### `good/`

| Trace | Exercises |
|-------|-----------|
| `http-session.jsonl` | Streamable HTTP session: session-ID assignment and echo, `MCP-Protocol-Version` headers, ping (TRAN-011/013/017/018 pass paths) |
| `stdio-feature-session.jsonl` | Every feature area conformant in one session: tools (incl. outputSchema + structuredContent), resources (read/blob/subscribe/updated), prompts (text/image/audio/embedded), logging, completion, pagination cursor flow |
| `stdio-full-session.jsonl` | Handshake plus ping, tools/list, tools/call over stdio |
| `stdio-minimal-init.jsonl` | Smallest conformant session: the three-message handshake |

### `violations/`

Each file injects exactly the violation its name states; `golden/` shows the full
expected report, including any intrinsic secondary findings the injected defect
causes (a malformed notification also fails lifecycle accounting, for example).

| Trace | Falsifies |
|-------|-----------|
| `base-001-request-id-boolean.jsonl` | BASE-001 |
| `base-002-request-id-null.jsonl` | BASE-002 |
| `base-003-request-id-reuse.jsonl` | BASE-003 |
| `base-004-result-unknown-id.jsonl` | BASE-004 |
| `base-005-notification-with-id.jsonl` | BASE-005 |
| `base-006-error-missing-message.jsonl` | BASE-006 |
| `base-007-error-code-float.jsonl` | BASE-007 |
| `base-008-jsonrpc-version.jsonl` | BASE-008 |
| `base-009-error-unknown-id.jsonl` | BASE-009 |
| `base-010-response-without-result.jsonl` | BASE-010 |
| `comp-001-capability-undeclared.jsonl` | COMP-001 |
| `life-001-first-message-not-initialize.jsonl` | LIFE-001 |
| `life-002-initialize-missing-protocolversion.jsonl` | LIFE-002 |
| `life-003-missing-initialized.jsonl` | LIFE-003 |
| `life-004-client-request-before-init-response.jsonl` | LIFE-004 |
| `life-005-server-request-before-initialized.jsonl` | LIFE-005 |
| `life-006-result-version-invalid.jsonl` | LIFE-006 |
| `life-007-initialize-protocolversion-not-string.jsonl` | LIFE-007 |
| `life-009-undeclared-capability-use.jsonl` | LIFE-009 |
| `log-001-capability-undeclared.jsonl` | LOG-001 |
| `page-002-cursor-never-issued.jsonl` | PAGE-002 |
| `prom-001-capability-undeclared.jsonl` | PROM-001 |
| `prom-003-image-data-invalid.jsonl` | PROM-003 |
| `prom-004-audio-data-invalid.jsonl` | PROM-004 |
| `prom-005-embedded-resource-malformed.jsonl` | PROM-005 |
| `res-001-capability-undeclared.jsonl` | RES-001 |
| `res-004-uri-bad-scheme.jsonl` | RES-004 |
| `res-006-blob-not-base64.jsonl` | RES-006 |
| `tool-001-capability-undeclared.jsonl` | TOOL-001 |
| `tool-003-input-schema-null.jsonl` | TOOL-003 |
| `tool-005-name-length.jsonl` | TOOL-005 |
| `tool-006-name-charset.jsonl` | TOOL-006 |
| `tool-008-name-duplicate.jsonl` | TOOL-008 |
| `tool-009-embedded-resource-without-capability.jsonl` | TOOL-009 |
| `tool-010-structured-without-text.jsonl` | TOOL-010 |
| `tool-011-output-schema-no-structured-result.jsonl` | TOOL-011 |
| `tran-004-stdout-invalid-message.jsonl` | TRAN-004 |
| `tran-005-stdin-invalid-message.jsonl` | TRAN-005 |
| `tran-011-session-id-invisible-ascii.jsonl` | TRAN-011 |
| `tran-013-session-id-not-echoed.jsonl` | TRAN-013 |
| `tran-017-protocol-version-header-missing.jsonl` | TRAN-017 |
| `tran-018-protocol-version-mismatched.jsonl` | TRAN-018 |
