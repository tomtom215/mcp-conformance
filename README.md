<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

A Rust-native conformance toolkit for the [Model Context Protocol](https://modelcontextprotocol.io)
(MCP): a machine-readable requirement registry, a transport-agnostic protocol trace
validator, a Rust "everything server" reference implementation, and a reference host runtime.

**Status: planning.** No code has shipped yet, and this README makes no claims a release
hasn't earned. The complete project plan — charter, verified ecosystem context, architecture,
conformance strategy, engineering standards, security model, roadmap, and decision records —
lives in [`docs/plan/`](docs/plan/README.md).

## Why this exists

Conformance is the load-bearing mechanism of MCP's maturity model: SEP-1730 gates SDK tier
standing on conformance pass rates, and SEP-2484 gates spec finalization on conformance
scenarios. The official suite executes live scenarios from TypeScript; nothing in any
language validates *recorded traces* of MCP traffic against the spec's normative
requirements, and no Rust everything server exists. This project builds that missing half —
upstream-first, calibrated against the official suite, engineered to the standard set by
[a2a-rust](https://github.com/tomtom215/a2a-rust).

The reasoning, with every claim verified and dated: [docs/plan/00-charter.md](docs/plan/00-charter.md).

## License

[MIT](LICENSE)
