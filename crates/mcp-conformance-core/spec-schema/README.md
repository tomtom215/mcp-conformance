<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Vendored protocol schemas

`<revision>.schema.json` is the Model Context Protocol's own JSON Schema for that
revision, copied byte for byte from the specification repository,
<https://github.com/modelcontextprotocol/modelcontextprotocol>, at commit
`3778656b2e0ba6725035e816fe3085374cbaa736` (2026-10-12):

| File | Upstream path | SHA-256 |
|------|---------------|---------|
| `2025-11-25.schema.json` | `schema/2025-11-25/schema.json` | `268a5f82ba70fd7e4b6dc4aa1e64f116f74b4d0edcb69dc046829c79dd4e97e7` |
| `2026-07-28.schema.json` | `schema/2026-07-28/schema.json` | `ef70b61f99b6d2e5e3b46863822eab08dff6a45bedc7a08914e0e5b133f40203` |

They are the specification's work, not this project's, and are distributed under
the specification's licence, reproduced in [`LICENSE`](LICENSE); that is why this
crate's licence is `MIT AND Apache-2.0`.

**Why `main` and not the release tag.** The dated directories on `main` carry
upstream's corrections made after the release: the `2025-11-25` copy at the
`2025-11-25` tag types `NumberSchema`'s `minimum`, `maximum` and `default` as
`integer`, which contradicts `schema.ts` (`number`), and `main` fixes it. The
specification text this project quotes is read from `main` for the same reason.

**Do not edit these files.** `mcp_conformance_core::wire_schema` records the two
places where the generated JSON Schema is stricter than `schema.ts`, the
specification's stated source of truth, and the validator corrects them when it
loads the schema; the files stay byte-identical to upstream so that
`cargo xtask spec-drift` can compare them with `main` byte for byte. To refresh
them, copy the new upstream files, update the commit here and in
`src/wire_schema.rs`, and re-check every erratum (a test fails on any erratum
whose upstream value no longer matches).
