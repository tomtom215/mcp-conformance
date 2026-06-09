<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Project Plan

**Status:** Active
**Last reviewed:** 2026-06-09
**Maintainer:** Tom F. ([@tomtom215](https://github.com/tomtom215))

---

The planning corpus for **mcp-conformance** — a Rust-native conformance toolkit for the
[Model Context Protocol](https://modelcontextprotocol.io) (MCP).

This directory is the project's single source of planning truth. It is a *living* set of
documents: modular by concern, updated in place, and governed by the documentation model in
[ADR-0001](decisions/0001-plan-documentation-model.md). Git history is the changelog; no
document in this tree carries inline update logs.

## Documents

| Doc | Owns | Read when |
|-----|------|-----------|
| [00-charter.md](00-charter.md) | Mission, thesis, goals, non-goals, success criteria | First. Everything else serves this. |
| [01-ecosystem-context.md](01-ecosystem-context.md) | Verified facts about MCP, the official conformance suite, the Rust SDK, and prior art — with sources and verification dates | Before citing any external fact, or when a fact looks stale |
| [02-architecture.md](02-architecture.md) | Workspace layout, crate boundaries, dependency rules, hard technical problems | Before writing or reviewing code |
| [03-conformance-strategy.md](03-conformance-strategy.md) | How this project relates to the official suite, SEP-1730 and SEP-2484; requirement registry and trace-validation semantics | Before adding a scenario, check, or requirement |
| [04-engineering-standards.md](04-engineering-standards.md) | The non-negotiable quality bar: lints, tests, CI gates, docs, releases | Before every PR |
| [05-security-model.md](05-security-model.md) | Threat model, default-secure posture, vulnerability handling | Before touching transports, parsers, or process spawning |
| [06-roadmap.md](06-roadmap.md) | Milestones with definitions of done, sequencing, and gates | To find out what to build next and when it counts as finished |
| [07-ecosystem-engagement.md](07-ecosystem-engagement.md) | Upstream-first policy, contribution backlog, adoption plan | Before opening anything against an upstream repo |
| [08-risk-register.md](08-risk-register.md) | Risks, mitigations, and the triggers that change this plan | At every milestone gate |
| [decisions/](decisions/README.md) | Architecture Decision Records (append-only) | When asking "why is it like this?" |

## Conventions (digest)

The full rules live in [ADR-0001](decisions/0001-plan-documentation-model.md). The five that
matter most:

1. **One concern per file.** New concerns get new files; no file becomes a dumping ground.
2. **Documents state current intent.** History lives in git, never in "Update:" banners or
   inline changelogs.
3. **Volatile facts live in one place** — the register in
   [01-ecosystem-context.md](01-ecosystem-context.md), each row carrying a verification date
   and source. Other documents link to the register instead of repeating numbers.
4. **Decisions are append-only.** Changing a decision means a new ADR that supersedes the old
   one, not an edit that erases the reasoning.
5. **Numbered prefixes are reading order, never reused.** Renumbering breaks links and is
   forbidden.

## Review policy

Every document carries a `Last reviewed` date. A document untouched for more than 90 days is
due for a review sweep: confirm it still describes current intent, re-verify any register rows
it depends on, and update the date. Reviews that change nothing still update the date — that
is the signal that someone looked.

## Glossary

| Term | Meaning |
|------|---------|
| **MCP** | Model Context Protocol — open protocol for connecting AI applications to tools and data; governed under the Agentic AI Foundation (Linux Foundation). |
| **Spec revision** | A dated protocol version, e.g. `2025-11-25` (current) or `2026-07-28` (release candidate). |
| **SEP** | Specification Enhancement Proposal — MCP's change process. |
| **SEP-1730** | The SDK Tiering System: Tier 1/2/3 classification of MCP SDKs by conformance and maintenance commitments. |
| **SEP-2484** | Requires a merged conformance scenario and a traceability file before a Standards-Track SEP can reach Final status. |
| **Official suite** | `@modelcontextprotocol/conformance` — the TypeScript conformance runner maintained in the MCP org. |
| **Everything server** | A reference server that exercises every protocol capability; SEP-1730 asks each SDK to carry one. |
| **SUT** | System under test — the client or server an evaluation runs against. |
| **Trace** | A recorded, ordered log of protocol traffic (messages plus transport events) suitable for offline validation. |
| **Requirement registry** | This project's machine-readable inventory of spec MUST/MUST NOT/SHOULD clauses, keyed by stable IDs. |
| **rmcp** | The official MCP Rust SDK crate, maintained in `modelcontextprotocol/rust-sdk`. |
| **RFC 2119** | The IETF keyword convention (MUST, SHOULD, MAY) the MCP spec uses for normative language. |

## License

MIT — see [LICENSE](../../LICENSE).
