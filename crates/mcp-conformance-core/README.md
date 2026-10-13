<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance-core

The Model Context Protocol specification as data: a requirement registry (normative
clauses with stable IDs, verbatim quotes, and SEP-2484-shaped check-or-exclusion
traceability), the recorded-trace event schema, JSON-RPC message classification, and
canonical JSON serialization. No I/O; every function is a pure transformation.

> **This README is `main`'s.** `cargo install` gets the latest release, `0.6.0`,
> whose own README is the one on crates.io. Documentation for the release is at
> <https://mcp-conformance.com>, and for `main` at <https://mcp-conformance.com/dev/>.
>
> **Not yet released** (on `main` since `0.6.0`):
>
> - the `2026-07-28` rows for `basic/patterns/cancellation` (`CANC-*`),
>   `basic/patterns/progress` (`PROG-*`) and `basic/patterns/index`, the `x-mcp-header`
>   behaviour table, and the requirement splits the changelog lists;
> - the documented form of `SourceRef::quote` for table rows.
Start at `requirement::RegistrySet::builtin()`, which loads the registries of every
revision the crate carries; `.registry(revision)` projects one of them.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance). Pre-1.0:
the registry's *format* is stable-by-intent; its *contents* cover every in-scope
normative clause of protocol revisions `2025-11-25` and `2026-07-28`, each judged
by checks or carrying a documented exclusion — "in-scope" is explicit data, not
judgment: each revision's `registry/<revision>/sources.json` lists the pages, and
`cargo xtask spec-drift` re-verifies every quote against the published text, and
every section anchor against the published page, on a schedule. See the
repository's `docs/plan/` for the roadmap.

License: MIT
