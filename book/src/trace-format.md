<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# The trace format

A trace is **JSON Lines**: one event per line. Each event carries a
capture-assigned `seq` (the only ordering authority — never inferred later), a
`direction` (`client-to-server` / `server-to-client`), a `transport`, and a
`kind`:

- **`message`** events hold the JSON-RPC payload (as parsed JSON: every value,
  not its whitespace, member order, or the spelling of its numbers);
- **`http`** events record the conformance-relevant headers, a response's
  status, and a client request's `method` — Streamable HTTP binds different
  obligations to `POST`, `GET`, and `DELETE`, so a clause addressed to one of
  them is judged only where the recording says which it was; and
- **`lifecycle`** events mark transport open/close.

The format is published as a JSON Schema (draft 2020-12),
[`trace-event.schema.json`](https://github.com/tomtom215/mcp-conformance/blob/main/crates/mcp-conformance-core/schema/trace-event.schema.json),
for recorders written in any language. It states every rule the validator's
reader applies to one record — a test runs both over every corpus record and one
violation of each rule — and says where JSON Schema cannot follow: the
document-level rules (one record per line, `seq` strictly increasing), and
integers written as `1.0`, which the reader refuses. Unknown members are ignored,
so producers may add their own.

The redaction rules that keep credential-bearing headers out of a trace by
construction are in
[`05-security-model.md`](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/05-security-model.md).

## One worked example

The example below is embedded verbatim from the
[README](https://github.com/tomtom215/mcp-conformance/blob/main/README.md), where
a test (`readme_examples.rs`) pins it to the validator's *real* output — so what
you read here cannot drift from what the tool actually produces. It is a session
that reuses a request ID, and the verdict that catches it:

{{#include ../../README.md:trace-example}}

The `totals` line distinguishes the verdict's components, and two of them are
not passes. The **not-applicable** rows are capability-gated requirements this
session never negotiated (the resources and prompts clauses); the
**not-observed** rows are clauses whose subject matter never appeared at all —
nothing was paginated, no binary content was sent, no error was returned. A
four-line session earns 17 passes, not a hundred. See
[Architecture](architecture.md) for why that distinction is load-bearing.
