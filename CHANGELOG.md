<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project are documented in this file.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0, minor releases may contain breaking changes; entries say so explicitly.

## [Unreleased]

### Added

- `mcp-conformance-core`: requirement registry model (RFC 2119 levels, verbatim
  spec quotes, SEP-2484-shaped check-or-exclusion traceability) seeded with 17
  requirements for protocol revision `2025-11-25`; JSON Lines trace event schema;
  JSON-RPC message classification; canonical JSON serialization with RFC 8785
  object-key ordering and exact float round-tripping.
- `mcp-trace-validator`: deterministic validation engine with 15 checks across the
  base-protocol and lifecycle requirement areas; human/JSON reports with
  pass/fail/warn/excluded/unsupported accounting; CLI with documented exit codes
  (0 pass, 1 findings, 2 invocation problem, 3 malformed trace); golden-corpus test
  harness with falsifiability enforcement (every check killed by a committed
  violation trace).
- `mcp-everything-server`: default-secure HTTP transport policy — loopback-only
  `Host`/`Origin` allowlisting, fail-closed parsing, explicit
  `dangerously_allow_any_host` opt-out.
- `mcp-reference-host`: deterministic retry/backoff policy with caller-supplied
  jitter and capped `Retry-After` honoring.
- Workspace tooling: `cargo xtask ci` (all local gates) and `cargo xtask bless`
  (golden regeneration); CI with format/clippy/test matrices (stable + MSRV 1.85 ×
  Linux/macOS/Windows × three feature modes), docs, `cargo-deny`, package
  validation, diff-scoped mutation gate on PRs, and scheduled RustSec audit + full
  mutation sweep.
