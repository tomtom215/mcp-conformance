<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0005: CLI Argument Parsing — clap, Isolated Behind a `cli` Feature

**Date:** 2026-06-09
**Status:** Accepted
**Author:** Tom F.

---

## Context

`mcp-trace-validator` is both a library (the validation engine other tools embed) and
a binary (the CLI users script in CI). The CLI needs flags, subcommands, generated
help, and stable error UX; the library must not impose any of that on its consumers.
The dependency philosophy ([04-engineering-standards.md](../04-engineering-standards.md))
puts the burden of proof on every dependency.

## Decision

Use **clap** (derive API, workspace-pinned `>=4.5, <5`) — but only behind a `cli`
feature that gates the binary target (`required-features = ["cli"]`):

- `default = ["cli"]` so `cargo install mcp-trace-validator` and `cargo run` work
  without flags.
- Library consumers use `default-features = false` and get an engine whose only
  dependencies are `mcp-conformance-core` + serde family. CI lints and tests the
  `--no-default-features` configuration to keep that true.

The CLI's exit codes (0/1/2/3) are a documented stable interface, pinned by
integration tests that execute the real binary (`tests/cli.rs`).

## Consequences

### Positive

- Professional CLI UX (help, errors, future shell completions) without hand-rolled
  parsing drift.
- The engine stays embeddable: no transitive clap in any library consumer.
- Feature-gating is verified by CI, not by promise.

### Negative

- clap is the workspace's heaviest dependency subtree; binary installs pay it.
- Two compilation configurations of the crate to keep green.

## Alternatives Considered

### Minimal parsers (`lexopt`, `pico-args`) or hand-rolled `std::env::args`

Rejected for the user-facing tool: help text, subcommands, and error UX would be
hand-maintained forever; the saved compile time does not buy back that maintenance.
(xtask, which is internal and trivial, *does* hand-roll its command dispatch —
the right tool differs with the audience.)

### clap as an unconditional dependency

Rejected: it would leak into every library consumer's tree, contradicting the
engine-is-embeddable goal and the dependency philosophy.
