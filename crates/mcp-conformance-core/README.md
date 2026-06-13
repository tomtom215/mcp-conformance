<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance-core

The Model Context Protocol specification as data: a requirement registry (normative
clauses with stable IDs, verbatim quotes, and SEP-2484-shaped check-or-exclusion
traceability), the recorded-trace event schema, JSON-RPC message classification, and
canonical JSON serialization. No I/O; every function is a pure transformation.
Start at `requirement::Registry::builtin_2025_11_25()`.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance). Pre-1.0:
the registry's *format* is stable-by-intent; its *contents* cover every in-scope
normative clause of protocol revision `2025-11-25`, each judged by checks or
carrying a documented exclusion — "in-scope" is explicit data, not judgment:
`registry/2025-11-25/sources.json` lists the pages, and `cargo xtask spec-drift`
re-verifies every quote against the published text on a schedule. See the
repository's `docs/plan/` for the roadmap.

License: MIT
