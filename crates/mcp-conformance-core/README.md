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
carrying a documented exclusion. See the repository's `docs/plan/` for the roadmap.

License: MIT
