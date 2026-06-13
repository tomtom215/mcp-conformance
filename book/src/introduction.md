<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Introduction

This is the reader's guide to **mcp-conformance**, an independent, trace-based
conformance toolkit for the [Model Context Protocol](https://modelcontextprotocol.io)
(MCP). It judges whether an MCP implementation conforms to a specific spec
revision (`2025-11-25` today) — and, just as importantly, it is built so that an
ecosystem with its own authoritative test suite has reason to trust the verdict.

> **Status: pre-release.** The crates are published on crates.io at `0.x`;
> the public API and the verdicts it produces may still change between minor
> releases, and the changelog says so explicitly when they do. This book tracks
> `main`.

## The one idea

The validator's input is a **trace**: an ordered, serialized record of a protocol
interaction. Its engine is a pure function — `&[TraceEvent] -> Report` — with no
network, no clock, and no I/O of its own. Capturing the trace is somebody else's
job; judging it is the validator's only job. That split is what makes a verdict
deterministic, replayable, language- and SDK-agnostic, and — because it is
reproducible from a committed file — auditable rather than a "trust us."

The credibility mechanism is the **agreement check**: on every CI run the
reference server is driven by the *official* conformance suite, the same sessions
are captured as traces, and this toolkit's verdicts are diffed against the
official runner's. Agreement is the default; an unexplained divergence fails the
build. A validator whose verdicts are checked against the recognized authority on
every commit is calibrated, not merely asserted to be correct.

## The crates

| Crate | What it is |
|-------|------------|
| [`mcp-conformance-core`](https://crates.io/crates/mcp-conformance-core) | The spec as data: the requirement registry, the trace schema, and RFC 8785 canonical JSON. Serde-only; no protocol SDK. |
| [`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator) | The deterministic judgment engine and its CLI (human / JSON / JUnit reports, documented exit codes). |
| [`mcp-everything-server`](https://crates.io/crates/mcp-everything-server) | A reference server on the official `rmcp` SDK that passes the suite's server scenarios, with a session tap that records traces for the agreement check. |
| [`mcp-reference-host`](https://crates.io/crates/mcp-reference-host) | A reference host (client) that passes the suite's client scenarios and captures host-side traces. |

## How this book is organized

Each chapter is a curated entry point. The
[planning documents](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/README.md)
— charter, ecosystem register, architecture, conformance strategy, engineering
standards, security model, roadmap, and decision records — remain the single
source of truth, and every chapter links to the authoritative document behind it.

- **[Architecture](architecture.md)** — why judge traces instead of live
  sessions, and the design trade-offs that follow.
- **[The trace format](trace-format.md)** — the JSON Lines schema, with a worked
  example embedded from the README so it cannot drift.
- **[The trace corpus](corpus.md)** — the golden good/violation fixtures and
  their provenance, embedded from `corpus/README.md`.
- **[Conformance results](conformance-results.md)** — the published tier-gap
  measurements of real SDKs against the official suite.
