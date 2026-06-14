<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# pmcp tier-gap report — `2025-11-25`

**What this is.** A reproducible measurement of the community Rust MCP SDK
[`pmcp`](https://crates.io/crates/pmcp) (repo
[`paiml/pmcp`](https://github.com/paiml/pmcp)) against the official conformance
suite's server scenarios, the requirement-level reading of where it falls short,
and a concrete close-the-gap checklist. It is the optional second stewardship
artifact ([roadmap M5](../plan/06-roadmap.md)) that the
[rmcp tier-gap report](rmcp-tier-gap-2025-11-25.md) proves the method generalizes;
the method is fully reproducible from the committed harness and the commands in
the last section.

**This is not a verdict handed down, and not an official tier.** The official
suite is the authority on conformance ([strategy §Position](../plan/03-conformance-strategy.md));
the numbers below are *its* numbers, captured by running it, with our registry
used only to read each failure back to a spec clause. pmcp is a *community* SDK
(not in the official tier table — [register 5.7](../plan/01-ecosystem-context.md))
and ships its own internal 19-scenario conformance engine; as far as we can tell
this is the first measurement of pmcp against the **official** suite.

## Method caveat: we built the system-under-test (read this first)

The rmcp report measured rmcp's **own** `conformance-server` — an artifact its
maintainers built and stand behind. **pmcp ships no suite-wired server and no
"everything"-style example** (verified 2026-06-14), so to measure it at all we
had to *write* the SUT: a small standalone server built on pmcp's public API
([`docs/reports/pmcp-harness/`](pmcp-harness/), a non-workspace project — pmcp's
MSRV 1.91 never enters this repo's 1.88 workspace). Two consequences, stated
plainly:

1. **This measures "what a faithful pmcp/Streamable-HTTP integration achieves
   against the suite," not "pmcp's own conformance posture."** A different
   integrator might wire a tool differently.
2. **To keep the number honest we iterated the harness until the only remaining
   failures were pmcp-attributable** — i.e. we fixed every failure that was *our*
   wiring (one: tool errors, see below) and re-ran. Every residual failure was
   then root-caused **twice** — empirically (the suite's `checks.json`) and by
   reading pmcp's source — and each is a limitation of pmcp's Streamable-HTTP
   surface, not of the harness. The harness source and all 30 raw `checks.json`
   files are committed so the attribution is auditable, not asserted.

## Measurement

| Field | Value |
|-------|-------|
| Subject | crates.io `pmcp` **2.9.0** (`paiml/pmcp`), served over its `StreamableHttpServer` (route `/`, loopback) |
| Harness | [`docs/reports/pmcp-harness/`](pmcp-harness/) (standalone; `pmcp` features `streamable-http,macros,schema-generation`) |
| Suite | `@modelcontextprotocol/conformance@0.1.16` (this repo's pin) |
| Spec revision | `2025-11-25` (`--spec-version`); the suite's `checks.json` `specReferences` link to its bundled `2025-06-18` doc anchors — the failing behaviors are revision-stable across that span |
| Toolchain / date | cargo 1.94.1, Node 22.22, 2026-06-14 |
| **Server scenarios** | **16 passed, 14 failed (16/30)** — 17/32 at individual-check granularity |

The figure was identical across three independent runs and was **recomputed
directly from the 30 committed `checks.json` files** ([`pmcp-harness/checks/`](pmcp-harness/checks/)),
not taken on trust. Scenario count (30) is the stable denominator; *check* counts
vary by SUT because a scenario that cannot proceed emits fewer checks (pmcp's
elicitation scenarios bail at one check where a capable server runs five), so the
scenario level is the unit comparable across SUTs.

Passing (16): `server-initialize`, `ping`, `logging-set-level`, `tools-list`,
`tools-call-simple-text`, `tools-call-error`, `resources-list`,
`resources-read-text`, `resources-templates-read`, `resources-subscribe`,
`resources-unsubscribe`, `prompts-list`, `prompts-get-simple`,
`prompts-get-with-args`, `prompts-get-with-image`,
`localhost-host-rebinding-rejected` (2/2 — the CVE-2026-42559 DNS-rebinding class
is handled).

Failing (14), grouped by the root cause established below:

| Scenario(s) | `checks.json` reason (verbatim) | Cause |
|-------------|----------------------------------|-------|
| `tools-call-with-logging` | "No log notifications received" | 1 |
| `tools-call-with-progress` | "No progress notifications received" | 1 |
| `tools-call-sampling` | "MCP error -32603 … Client does not support sampling (no peer back-channel)" | 1 |
| `tools-call-elicitation`, `elicitation-sep1034-*`, `elicitation-sep1330-*` | "Server did not request elicitation from client" | 1 |
| `tools-call-image` / `-audio` / `-embedded-resource` / `-mixed-content` | "No image/audio/resource content found"; "Expected multiple content items" | 2 |
| `prompts-get-embedded-resource` | Zod `invalid_union` on the message content | 3 |
| `resources-read-binary` | "Content missing blob field" (`hasBlob: false`) | 4 |
| `completion-complete` | Zod `invalid_type` — `completion` expected object | 5 |
| `server-accepts-multiple-post-streams` | "Server rejected some requests. Statuses: 400, 400, 400" | 6 |

## Requirement-level reading — six root causes, all pmcp-attributable

Each cause was confirmed both over the wire and by reading pmcp 2.9.0 source.

1. **No server→client back-channel over Streamable HTTP** (6 scenarios:
   logging, progress, sampling, all three elicitation). pmcp wires
   `notification_tx`, the `ServerRequestDispatcher`, and the peer handle **only**
   inside `Server::run::<T: Transport>()` (the stdio/WebSocket loop,
   `server/mod.rs:861-895`). `StreamableHttpServer` never calls `run()` — it locks
   the `Server` and calls `handle_request` directly — so a tool's `extra.peer()`
   is `None`, `report_progress()` has no sink, and `pmcp::log()` has no sink.
   `ServerBuilder::build()` sets all three fields to `None`
   (`mod.rs:3632/3637/3638`) and no public API assigns them outside `run()`.
   Separately, pmcp's `PeerHandle` exposes only `sample`/`list_roots`/
   `progress_notify` — there is **no elicitation primitive at all**, and
   `ElicitationManager`'s request channel is wired only in a unit test. **This
   group is transport-scoped:** the same handlers would get a live peer under
   stdio/WebSocket; the gap is specifically pmcp's HTTP transport.
2. **`ToolHandler` output is stringified** (4 scenarios: image, audio,
   embedded-resource, mixed-content). Both dispatch paths take the handler's
   `serde_json::Value` and wrap the **whole thing** as one text block
   (`mod.rs:1347`: `CallToolResult::new(vec![Content::text(result.to_string())])`).
   A tool therefore cannot emit a structured `content` array with image/audio/
   resource items — confirmed on the wire (a returned `{"content":[…]}` came back
   as a single text block containing the escaped JSON). (`tools-call-simple-text`
   passes precisely because a stringified blob still satisfies "a text block
   exists.")
3. **`Content::Resource` serializes flat, not spec-nested** (`prompts-get-embedded-resource`).
   pmcp emits `{"type":"resource","uri":…,"text":…}`; the spec / TS-SDK require
   `{"type":"resource","resource":{…}}`, and the MCP client's schema rejects the
   flat form (`invalid_union`). `prompts-get-with-image` passes because
   `Content::Image` serializes correctly and prompts return a typed
   `GetPromptResult` (not stringified) — so the delta is purely the resource shape.
4. **No reachable `blob` for binary resource reads** (`resources-read-binary`).
   `Content::Resource` has only a `text` field; `resource_contents_serde` never
   emits `blob`. *Precise nuance:* pmcp does define a `blob` field on one type —
   `UIResourceContents` (`ui.rs:230`) — but that is a standalone MCP-Apps UI type,
   **not** a `Content` variant, and is unreachable from `ReadResourceResult`,
   `PromptMessage`, or tool results. So binary blob reads cannot match the wire
   shape on those three surfaces; it is not that pmcp lacks a `blob` field
   anywhere.
5. **`completion/complete` is stubbed** (`completion-complete`). The streamable
   `Server` answers `Complete | Subscribe | Unsubscribe | SetLoggingLevel | Ping`
   with a hardcoded `Ok(json!({}))` (`mod.rs:1156-1160`); the suite needs
   `result.completion.values[]`. (Subscribe/unsubscribe/set-level/ping pass
   precisely because `{}` is acceptable for *them*.)
6. **Strict `mcp-protocol-version` exact-match** (`server-accepts-multiple-post-streams`).
   `validate_protocol_version_matches_session` (`streamable_http_server.rs:710`)
   returns HTTP 400 unless the header equals the negotiated version. The scenario
   deliberately sends `mcp-protocol-version: 2025-03-26` (a *supported* version)
   on a session negotiated at `2025-11-25`, so all three concurrent POSTs get 400.
   The config exposes no field to relax exact-match to "any supported version";
   rmcp and the TS-SDK accept it.

The reading that matters for adopters: pmcp 2.9.0 is solid on the **stateless
request/response surface** the suite exercises — lifecycle, tool/resource/prompt
*listing*, simple text tools, errors, text resources, template reads, prompt
argument substitution, and DNS-rebinding protection all pass — and its gaps
cluster in two places: **anything needing a server→client message over HTTP**
(notifications, sampling, elicitation — transport-scoped, present on stdio) and
**structured content shapes** (multi-content tool results, nested embedded
resources, binary blobs, completion values — pmcp-wide).

## Close-the-gap checklist (for pmcp)

1. **Wire the back-channel under `StreamableHttpServer`** (closes 6 scenarios).
   The dispatcher/peer/notification plumbing already exists for `Server::run`;
   the HTTP server needs to populate `notification_tx` + a peer (draining server
   requests onto the SSE channel it already maintains, and routing POSTed
   responses back into a dispatcher). Add an `elicit` primitive to `PeerHandle`.
2. **Stop stringifying tool output** (closes 4 scenarios). Detect a
   `{ "content": [...] }`-shaped `Value` (or accept a typed `CallToolResult`) and
   pass the content array through instead of `to_string()`-ing it.
3. **Nest `Content::Resource` and add a `blob` arm** (closes 2 scenarios) to
   match `{"type":"resource","resource":{…}}` and binary `blob` reads.
4. **Implement `completion/complete`** (closes 1 scenario) — route to the
   existing `completable` types instead of returning `{}`.
5. **Relax protocol-version matching** (closes 1 scenario) to accept any
   *supported* version on a request, not only an exact session match.

Items 2–5 are localized serialization/dispatch fixes; item 1 is the substantial
one. None requires a protocol change — the suite is describing wire shapes pmcp
can already nearly produce.

## Reproducible method

The harness is committed at [`docs/reports/pmcp-harness/`](pmcp-harness/) (a
standalone, non-workspace cargo project) and the raw per-scenario verdicts at
[`docs/reports/pmcp-harness/checks/`](pmcp-harness/checks/).

```sh
# 1. Build the pmcp SUT (standalone — NOT part of this repo's workspace).
cp -r docs/reports/pmcp-harness /tmp/pmcp-harness && cd /tmp/pmcp-harness
cargo build                                  # resolves pmcp 2.9.0 from crates.io

# 2. Serve it on loopback (pmcp's StreamableHttpServer route is `/`, not `/mcp`).
PORT=8484 ./target/debug/pmcp-conformance-harness &

# 3. Run the pinned official suite's server scenarios.
npx -y @modelcontextprotocol/conformance@0.1.16 server \
  --url http://127.0.0.1:8484/ --spec-version 2025-11-25 --output-dir ./out

# 4. Recompute the score from the raw verdicts (what this report did).
#    Each ./out/<scenario>/checks.json is an array of {status: SUCCESS|FAILURE}.
```

Refresh this report when the suite pin moves
([suite-version policy](../plan/03-conformance-strategy.md)) or when a pmcp
release changes any of the six causes above. For context, the
[rmcp report](rmcp-tier-gap-2025-11-25.md) records 38/40 at check granularity
(two failing scenarios) and this repo's own everything-server passes every
scenario — the spread is the point: the same method, the same suite, three
different SDKs, three honestly-measured results.
