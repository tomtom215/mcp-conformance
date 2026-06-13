<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# The trace format

A trace is **JSON Lines**: one event per line. Each event carries a
capture-assigned `seq` (the only ordering authority — never inferred later), a
`direction` (`client-to-server` / `server-to-client`), a `transport`, and a
`kind`:

- **`message`** events hold the JSON-RPC payload verbatim;
- **`http`** events record the status and the conformance-relevant headers; and
- **`lifecycle`** events mark transport open/close.

The full schema, including the redaction rules that keep credential-bearing
headers out of a trace by construction, is in
[`02-architecture.md`](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/02-architecture.md)
and [`05-security-model.md`](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/05-security-model.md).

## One worked example

The example below is embedded verbatim from the
[README](https://github.com/tomtom215/mcp-conformance/blob/main/README.md), where
a test (`readme_examples.rs`) pins it to the validator's *real* output — so what
you read here cannot drift from what the tool actually produces. It is a session
that reuses a request ID, and the verdict that catches it:

{{#include ../../README.md:trace-example}}

The `totals` line distinguishes the verdict's components: the five
**not-applicable** rows are capability-gated requirements this session never
negotiated (the resources and prompts clauses) — reported as such, never as
passes. See [Architecture](architecture.md) for why that distinction is
load-bearing.
