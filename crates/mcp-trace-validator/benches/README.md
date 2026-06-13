<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Benchmarks

`cargo bench -p mcp-trace-validator` (and `-p mcp-conformance-core` for
canonicalization) measures:

- **`validator/validate_1003_events`** — full engine throughput (events/second):
  registry × trace → report over a synthetic 1003-event conformant session.
- **`validator/context_1003_events`** — context construction alone: message
  classification, lifecycle state-machine stepping, and request/response pairing.
- **`canonical/tool_result_payload`** — RFC 8785 canonicalization throughput
  (bytes/second) over a representative MCP tool-result payload.

## No regression gate — a recorded decision

These benchmarks measure and print; CI does not compare them against thresholds or
history. Gating needs a baseline corpus of measurements from pinned hardware, and
this project has neither accumulated history nor a dedicated runner — a threshold
invented today would be folklore, and folklore gates rot into `continue-on-error`.

**History does accumulate now** (third audit, 2026-06-13): the weekly scheduled
job runs `cargo bench --workspace` and uploads `target/criterion/` as a 90-day
artifact (`bench-history-<run-id>`), so the baseline corpus this decision was
waiting for builds itself — on shared runners, so cross-run noise stays part of
any future threshold conversation. Complexity regressions are separately gated
by the validator's pathological-input tests, which is where a real blow-up
fails loudly today.

Criterion runs with default features off: no rayon, no HTML/plotters reports —
console output only.
