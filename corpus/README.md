<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Trace corpus

Fixtures for the golden-corpus tests (`crates/mcp-trace-validator/tests/golden.rs`):

- **`good/`** — sessions that must validate with verdict `pass`.
- **`violations/`** — single-issue sessions, each falsifying at least one check;
  named after the requirement whose check they exist to kill.
- **`draft/{good,violations}/`** — the same two kinds for revision `2026-07-28`,
  judged against that revision's registry behind the `draft-2026-07-28` feature.
- **`draft/captured/`** — sessions recorded off the wire from implementations
  this repository did not write. No verdict is asserted for them; their whole
  report is byte-pinned instead. See *Captured versus authored* below.
- **`golden/`** — the byte-pinned expected report for every trace, with
  `golden/draft/` holding the `2026-07-28` corpus's. Regenerate only via
  `cargo xtask bless` and review the diff like code.

The revisions keep separate golden directories because both name traces after
requirement IDs drawn from their *own* registry: `base-045-…` is one clause at
`2025-11-25` and a different one at `2026-07-28`, so a shared directory would
let one revision's golden answer for the other's trace.

## Violation naming contract

A violation trace is named `area-nnn-<slug>.jsonl` and **must** produce a
Fail/Warn row with findings for exactly requirement `AREA-NNN` — the golden
harness enforces the attribution by name
(`violation_traces_fail_and_match_goldens`, and
`draft::draft_violation_traces_fail_and_match_goldens` for `draft/`), so a
defect re-routed to a different requirement cannot re-bless silently. A stem
that does not begin with a requirement ID fails the suite loudly.

## Captured versus authored

Everything in `good/`, `violations/` and `draft/{good,violations}/` is
**authored**: hand-written to exercise one clause. That is what makes them
precise, and it is also their limit — an authored fixture can only ever confirm
its author's reading of the specification. A check that is wrong in the same way
the author is wrong passes its unit tests and its corpus alike, and neither
notices.

`draft/captured/` exists to close that gap. Its traces are real recordings of
traffic between implementations neither of which was written to satisfy these
checks, so they can disagree with us — and when they do, the disagreement is
information rather than a test failure. Their goldens are pinned without any
pass/fail expectation, because a real implementation is whatever it actually is;
what the golden protects is *which requirements fire, on which events, with what
detail*. A check that starts misfiring on real traffic moves it.

## Provenance ledger

Every trace's origin, in one reviewable place that survives history rewrites (the
invariant test in `golden.rs` fails if a trace is added without a ledger row). All
current traces share one provenance: **hand-authored for this repository as
synthetic sessions** (no third-party traffic, no recorded production data),
written against the `2025-11-25` spec text fetched live from
modelcontextprotocol.io on 2026-06-09 (re-verified clause-by-clause against the
live text on 2026-06-11) and validated against the embedded registry
at the commit that introduced them.

Traces under `draft/captured/` are the exception and record their own origin
below: the implementations on both ends, the tool that recorded them, and when.

### `good/`

| Trace | Exercises |
|-------|-----------|
| `http-session.jsonl` | Streamable HTTP session: session-ID assignment and echo, `MCP-Protocol-Version` headers, `Accept`/`Content-Type` discipline, ping (TRAN-011/013/017/018/025/029/039/040 pass paths) |
| `stdio-feature-session.jsonl` | Every feature area conformant in one session: tools (incl. outputSchema + structuredContent), resources (read/blob/subscribe/updated), prompts (text/image/audio/embedded), logging, completion, pagination cursor flow |
| `stdio-full-session.jsonl` | Handshake plus ping, tools/list, tools/call over stdio |
| `stdio-minimal-init.jsonl` | Smallest conformant session: the three-message handshake |

### `violations/`

Each file injects exactly the violation its name states; `golden/` shows the full
expected report, including any intrinsic secondary findings the injected defect
causes (a malformed notification also fails lifecycle accounting, for example).

| Trace | Falsifies |
|-------|-----------|
| `base-001-request-id-boolean.jsonl` | BASE-001 |
| `base-002-request-id-null.jsonl` | BASE-002 |
| `base-003-request-id-reuse.jsonl` | BASE-003 |
| `base-004-request-answered-twice-cross-flavor.jsonl` | BASE-004 (the second of an error+result double-answer) |
| `base-004-result-unknown-id.jsonl` | BASE-004 |
| `base-005-notification-with-id.jsonl` | BASE-005 |
| `base-006-error-missing-message.jsonl` | BASE-006 |
| `base-007-error-code-float.jsonl` | BASE-007 |
| `base-008-jsonrpc-version.jsonl` | BASE-008 |
| `base-009-error-unknown-id.jsonl` | BASE-009 |
| `base-010-response-without-result.jsonl` | BASE-010 |
| `base-019-meta-key-bad-prefix.jsonl` | BASE-019, BASE-020 (shared `base.meta-key-format` check) |
| `comp-001-capability-undeclared.jsonl` | COMP-001 |
| `life-001-first-message-not-initialize.jsonl` | LIFE-001 |
| `life-002-initialize-missing-protocolversion.jsonl` | LIFE-002 |
| `life-003-missing-initialized.jsonl` | LIFE-003 |
| `life-004-client-request-before-init-response.jsonl` | LIFE-004 |
| `life-005-server-request-before-initialized.jsonl` | LIFE-005 |
| `life-006-result-version-invalid.jsonl` | LIFE-006 |
| `life-007-initialize-protocolversion-not-string.jsonl` | LIFE-007 |
| `life-009-undeclared-capability-use.jsonl` | LIFE-009 |
| `life-010-initialize-result-missing-capabilities.jsonl` | LIFE-010 |
| `log-001-capability-undeclared.jsonl` | LOG-001 |
| `page-002-cursor-never-issued.jsonl` | PAGE-002 |
| `prom-001-capability-undeclared.jsonl` | PROM-001 |
| `prom-003-image-data-invalid.jsonl` | PROM-003 |
| `prom-004-audio-data-invalid.jsonl` | PROM-004 |
| `prom-005-embedded-resource-malformed.jsonl` | PROM-005 |
| `prom-008-required-argument-unvalidated.jsonl` | PROM-008 |
| `res-001-capability-undeclared.jsonl` | RES-001 |
| `res-004-uri-bad-scheme.jsonl` | RES-004 |
| `res-006-blob-not-base64.jsonl` | RES-006 |
| `tool-001-capability-undeclared.jsonl` | TOOL-001 |
| `tool-003-input-schema-null.jsonl` | TOOL-003 |
| `tool-005-name-length.jsonl` | TOOL-005 |
| `tool-006-name-charset.jsonl` | TOOL-006 |
| `tool-008-name-duplicate.jsonl` | TOOL-008 |
| `tool-009-embedded-resource-without-capability.jsonl` | TOOL-009 |
| `tool-010-structured-without-text.jsonl` | TOOL-010 |
| `tool-011-output-schema-no-structured-result.jsonl` | TOOL-011 |
| `tran-004-stdout-invalid-message.jsonl` | TRAN-004 |
| `tran-005-stdin-invalid-message.jsonl` | TRAN-005 |
| `tran-011-session-id-invisible-ascii.jsonl` | TRAN-011 |
| `tran-013-session-id-not-echoed.jsonl` | TRAN-013 |
| `tran-017-protocol-version-header-missing.jsonl` | TRAN-017 |
| `tran-018-protocol-version-mismatched.jsonl` | TRAN-018 |
| `tran-025-accept-header-missing.jsonl` | TRAN-025, TRAN-039 (shared `transport.client-accept-header` check) |
| `tran-026-http-post-batch.jsonl` | TRAN-026 (a batch array POSTed after a clean handshake) |
| `tran-029-content-type-unexpected.jsonl` | TRAN-029, TRAN-040 (shared `transport.success-content-type` check) |

### `2026-07-28` captured (`corpus/draft/captured/`)

| Field | Value |
|---|---|
| Trace | `official-suite-2026-07-28-scenarios.jsonl` |
| Client | The **official MCP conformance suite**, `0.2.0-alpha.9`, driving its `2026-07-28` scenario set (the pin `cargo xtask draft-readiness` holds) |
| Server | This workspace's `mcp-everything-server`, which implements **`2025-11-25`** — so genuine non-conformance at `2026-07-28` is the expected content, not a defect in the recording |
| Recorded by | `mcp-everything-server`'s tap, during `cargo xtask draft-readiness`, 2026-08-17 |
| Contents | 91 events across 22 POST exchanges — `server/discover`, `tools/list`, `tools/call`, `completion/complete`, `resources/{list,read}`, `prompts/{list,get}`, progress notifications — every request carrying the revision's per-request `_meta` envelope, and no `initialize` anywhere |

**What it found, and why each finding is real.** Judged by the `2026-07-28`
registry it reports 121 pass, 2 fail, 1 warn, 148 excluded. All three findings
were checked against the recorded bytes:

- **TRAN-058** — all 22 client POSTs carry only `accept`, `content-type`,
  `host`, `mcp-protocol-version` and `origin`. The revision requires the
  request-metadata headers (`Mcp-Method`, and `Mcp-Name` where the method
  defines one). This is a finding about the *official suite's client*, not about
  our server.
- **TRAN-068** — all 22 SSE responses carry `content-type: text/event-stream`
  and nothing else; `X-Accel-Buffering: no` is absent.
- **CACH-001** — the `complete` results carry no `ttlMs`.

The last two are our `2025-11-25` server being held to a revision it does not
implement, which is the correct answer. **No finding was a false positive**, and
121 requirements passed on traffic nobody here authored — which is the only
evidence the authored fixtures cannot supply.

Regenerate with `cargo xtask draft-readiness` and copy
`target/draft-readiness/tap/001-stateless.jsonl`; the ephemeral port in the
`host` header changes per run, so re-copying is a deliberate act with a golden
diff, not something to do casually.

### `2026-07-28` authored (`corpus/draft/`)

Fixtures for the in-progress revision, judged by the `2026-07-28` registry
behind the `draft-2026-07-28` feature. Hand-authored rather than captured: no
SDK yet serves the stateless surface end to end, so these are minimal traces
written to exercise one clause each, which is also what keeps a violation
attributable to the check it kills.

Where a trace falsifies more than one requirement, the row says so and why.
There are only two reasons, and neither is sloppy authorship: several clauses
state *one* rule across several sections and share a check by design, or one
clause's antecedent is itself a violation by the other party — a server can
only fail to reject a bad header if the client sent one. Every other row
falsifies exactly the requirement it is named for.

| Trace | Exercises |
|-------|-----------|
| `stateless-session.jsonl` | Conformant `2026-07-28` stateless session over **stdio**: every request carries its own `_meta` envelope (protocolVersion + clientCapabilities), every result carries `resultType`, request ids are reused only after their response. It now also carries a complete conforming MRTR round — `input_required` with an `elicitation/create` request and a `requestState`, then a retry under a new id echoing the state and supplying `inputResponses` — so the MRTR checks pass on real content rather than by abstention. The Streamable HTTP clauses among them still pass by abstention — there is no HTTP framing for them to read, which is why `streamable-http-session.jsonl` exists. |
| `base-030-request-meta-missing-required-field.jsonl` | Request `_meta` omits `io.modelcontextprotocol/clientCapabilities` (BASE-030) |
| `base-031-malformed-meta-answered-with-result.jsonl` | Server answers a `_meta`-incomplete request with a result instead of `-32602` (BASE-031) |
| `base-032-invalid-params-not-http-400.jsonl` | `-32602` returned with HTTP 200 rather than 400 (BASE-032) |
| `base-034-input-request-for-undeclared-capability.jsonl` | Server returns `input_required` asking for `elicitation/create` the request never declared (BASE-034) |
| `base-035-missing-capability-error-without-list.jsonl` | `-32021` carries no `data.requiredCapabilities` (BASE-035) |
| `base-036-missing-capability-not-http-400.jsonl` | `-32021` returned with HTTP 500 rather than 400 (BASE-036) |
| `base-039-subscription-notification-untagged.jsonl` | Notification on a `subscriptions/listen` stream with no `io.modelcontextprotocol/subscriptionId` (BASE-039) |
| `base-040-malformed-traceparent.jsonl` | `traceparent` that is not W3C Trace Context shaped (BASE-040) |
| `base-045-request-id-reused-in-flight.jsonl` | Request id reused while the first is still outstanding (BASE-045) — legal at `2025-11-25` only after a response, and this trace reuses *before* one |
| `base-048-result-without-result-type.jsonl` | Result omits the `resultType` SEP-2322 requires (BASE-048) |
| `base-055-legacy-error-code.jsonl` | Error code `-32010` from the closed legacy sub-range (BASE-055) |
| `base-057-undefined-reserved-error-code.jsonl` | Error code `-32055`: inside the MCP-reserved sub-range but undefined (BASE-057) |
| `base-058-withdrawn-error-code.jsonl` | Error code `-32002`, withdrawn by this revision (BASE-058) |
| `base-060-app-code-in-reserved-range.jsonl` | Application-defined `-32500` placed inside the JSON-RPC reserved range (BASE-060) |
| `streamable-http-session.jsonl` | Conformant `2026-07-28` Streamable HTTP session: `server/discover`, a `tools/list` declaring an `x-mcp-header` annotation, a `tools/call` mirroring it into `Mcp-Param-Region` over an SSE response with `X-Accel-Buffering: no`, and a `resources/read` whose non-ASCII `Mcp-Name` rides the Base64 sentinel. Passes all 51 checked entries. |
| `tran-058-request-metadata-headers-missing.jsonl` | POST carries neither `Mcp-Method` nor `Mcp-Name` (TRAN-058) |
| `tran-060-client-posts-a-response.jsonl` | Client POSTs a JSON-RPC response (TRAN-060). Also falsifies BASE-046: at this revision a server cannot issue the request such a response would answer, so an unsolicited id is the only shape the violation can take. |
| `tran-066-independent-server-request.jsonl` | Server sends `elicitation/create` as its own request on the response stream instead of an MRTR input request (TRAN-066) |
| `tran-068-sse-without-accel-buffering.jsonl` | SSE response omits `X-Accel-Buffering: no` (TRAN-068, SHOULD → warn) |
| `tran-070-message-after-stream-close.jsonl` | Server answers a request whose response stream had already closed, which this revision treats as cancellation (TRAN-070) |
| `tran-071-protocol-version-header-missing.jsonl` | POST request without `MCP-Protocol-Version` (TRAN-071) |
| `tran-072-protocol-version-header-mismatched.jsonl` | Header says `2025-11-25`, body `_meta` says `2026-07-28`; the server rejects it correctly, isolating the client's fault (TRAN-072) |
| `tran-073-header-mismatch-not-rejected.jsonl` | The same disagreement, answered with a result (TRAN-073). Necessarily also falsifies TRAN-072 — the client fault *is* this clause's antecedent. |
| `tran-074-unsupported-version-without-supported-list.jsonl` | `-32022` without the `data.supported` list the clause requires (TRAN-074) |
| `tran-074-unsupported-version-accepted.jsonl` | A request naming a version the server's own `server/discover` result omits, answered with a result (TRAN-074) |
| `tran-075-method-not-found-not-404.jsonl` | `-32601` returned with HTTP 200 rather than 404 (TRAN-075) |
| `tran-077-mcp-name-unencoded.jsonl` | Non-ASCII `Mcp-Name` carried unencoded (TRAN-077, TRAN-086, TRAN-087 — one rule stated in three sections, one shared `transport.header-value-encoding` check) |
| `tran-079-x-mcp-header-not-mirrored.jsonl` | `tools/call` supplies a designated argument without its `Mcp-Param-*` header (TRAN-079) |
| `tran-080-x-mcp-header-name-invalid.jsonl` | One `inputSchema` with three annotation faults: a non-token name, a case-insensitive duplicate, and an annotation on a `number` property (TRAN-080) |
| `tran-089-sentinel-markers-miscased.jsonl` | `=?BASE64?…?=` — the sentinel markers must be exactly lowercase (TRAN-089). The server rejects it correctly, so nothing else fires. |
| `tran-092-sentinel-pattern-unencoded.jsonl` | A plain value that matches the sentinel pattern, carried verbatim rather than encoded (TRAN-092) |
| `tran-096-invalid-param-header-accepted.jsonl` | A recognized `Mcp-Param-Region` whose value has leading whitespace, answered with a result (TRAN-096). Also falsifies TRAN-077/086/087 — the unencodable value the server had to reject is itself the client's encoding fault. |
| `tran-097-header-body-mismatch-accepted.jsonl` | `Mcp-Param-Region: us-east1` against `arguments.region = "us-west1"`, answered with a result (TRAN-097, TRAN-100 — one rule stated in two sections) |
| `tran-098-header-mismatch-without-400.jsonl` | `HeaderMismatch` returned with HTTP 500 rather than 400 (TRAN-098, TRAN-102 — one rule stated in two sections) |
| `tran-074-unsupported-version-without-400.jsonl` | `-32022` returned with HTTP 200 rather than 400 (TRAN-074). The status half of that clause had no trace of its own until `transport.unsupported-version-status` was split out; it had been riding the kills of the sibling rules it was bundled with. |
| `tool-019-tools-undeclared.jsonl` | `tools/call` answered though discovery declared no `tools` capability (TOOL-019) |
| `tool-020-declared-tools-list-unimplemented.jsonl` | `tools` declared, but `tools/list` refused with `-32601` (TOOL-020) |
| `tool-022-tools-list-order-changes.jsonl` | Two `tools/list` results with the same tools in a different order (TOOL-022) |
| `tool-034-mirrored-integer-out-of-range.jsonl` | An `x-mcp-header`-annotated argument of 2^53, outside the IEEE 754 safe range (TOOL-034) |
| `tool-038-embedded-resource-undeclared.jsonl` | A `tools/call` result embedding a resource with no `resources` capability declared (TOOL-038) |
| `res-012-resources-undeclared.jsonl` | `resources/read` answered though discovery declared no `resources` capability (RES-012) |
| `res-013-declared-resources-list-unimplemented.jsonl` | `resources` declared, but `resources/list` refused with `-32601` (RES-013) |
| `res-022-read-empty-contents.jsonl` | `resources/read` answered with an empty `contents` array (RES-022) |
| `prom-012-prompts-undeclared.jsonl` | `prompts/get` answered though discovery declared no `prompts` capability (PROM-012) |
| `prom-013-declared-prompts-list-unimplemented.jsonl` | `prompts` declared, but `prompts/list` refused with `-32601` (PROM-013) |
| `comp-007-completions-undeclared.jsonl` | `completion/complete` answered though the `server/discover` result declared no `completions` capability (COMP-007) |
| `log-007-logging-undeclared.jsonl` | `notifications/message` emitted though discovery declared no `logging` capability (LOG-007) |
| `log-008-log-without-requested-level.jsonl` | A log notification in a session where no request set `io.modelcontextprotocol/logLevel` (LOG-008) |
| `log-009-log-on-subscription-stream.jsonl` | A log notification tagged with a subscription id, so travelling on a subscription's stream (LOG-009) |
| `log-010-unrecognized-log-level-accepted.jsonl` | A request declaring log level `verbose`, served rather than rejected with `-32602` (LOG-010) |
| `page-011-unissued-cursor-accepted.jsonl` | A `tools/list` presenting a cursor the session never issued, answered with a result (PAGE-011). Also falsifies PAGE-002 at its own revision's registry, and PAGE-010 here — the fabricated cursor is the client's defect and this clause's antecedent. |
| `cach-001-cacheable-result-without-hints.jsonl` | A `complete` `tools/list` result with no `ttlMs` caching hint (CACH-001) |
| `cach-008-negative-ttl.jsonl` | `ttlMs: -1`, which servers must never provide (CACH-008) |
| `cach-015-page-scope-changes.jsonl` | A paginated `tools/list` whose second page switches from `private` to `public` (CACH-015, and CACH-016 — the same rule and its worked example) |
| `subs-001-unrequested-notification-type.jsonl` | A `prompts/list_changed` on a subscription whose filter asked only for tools-list changes (SUBS-001) |
| `subs-002-notification-before-acknowledgment.jsonl` | A notification ahead of `notifications/subscriptions/acknowledged` on the same subscription id (SUBS-002) |
| `subs-006-graceful-close-result-not-empty.jsonl` | A graceful-closure response carrying `delivered` alongside `resultType` (SUBS-006, and SUBS-005 — one rule stated in the cancellation list and again under Graceful Closure) |
| `mrtr-004-input-required-on-unsupported-method.jsonl` | `input_required` answering a `resources/list`, which is not one of the three requests that may draw one (MRTR-004) |
| `mrtr-006-input-request-unknown-method.jsonl` | `inputRequests` asking for `tools/list`, which is not an ElicitRequest, CreateMessageRequest or ListRootsRequest (MRTR-006) |
| `mrtr-011-input-required-empty.jsonl` | `input_required` carrying neither `inputRequests` nor `requestState`, opening a round that cannot be completed (MRTR-011) |
| `mrtr-015-retry-without-input-responses.jsonl` | Retry echoes the state but supplies no `inputResponses` for the input it was asked for (MRTR-015) |
| `mrtr-016-request-state-not-echoed.jsonl` | Retry rewrites `requestState` instead of echoing it (MRTR-016). Also falsifies MRTR-003 and MRTR-017 by design — the same rule stated from the other side, "MUST NOT modify", sharing one check. |
| `mrtr-018-unsolicited-request-state.jsonl` | Retry invents a `requestState` for a round that issued none (MRTR-018) |
| `mrtr-019-retry-reuses-id.jsonl` | Retry reuses the original request's JSON-RPC id (MRTR-019) — legal under BASE-045, since the first was already answered, and forbidden here |
| `mrtr-020-request-state-on-another-method.jsonl` | A `prompts/get` carrying the `requestState` a `tools/call` round issued (MRTR-020) |
| `mrtr-024-shortfall-answered-with-error.jsonl` | Retry omits requested input and the server answers `-32602` rather than asking again (MRTR-024). Necessarily also falsifies MRTR-015 — the client's shortfall is this clause's antecedent. |
| `tran-123-cancellation-without-request-id.jsonl` | `notifications/cancelled` carrying only a reason, naming no request to cancel (TRAN-123) |
| `tran-124-message-after-cancel-notification.jsonl` | Server answers a request the client cancelled by notification (TRAN-124). stdio's cancellation signal is the notification, not a stream close, so `transport.no-messages-after-cancellation` — which anchors on a `transport-close` — cannot see this and would have passed it vacuously. |
| `disc-001-server-discover-method-not-found.jsonl` | `server/discover` answered with `-32601`, which this revision makes mandatory (DISC-001) |
| `disc-002-dual-era-client-skips-probe.jsonl` | A client that speaks both eras — a modern `_meta` request, then an `initialize` fallback — whose first request is `tools/list` rather than the `server/discover` probe (DISC-002). The `initialize` carries a full `_meta` envelope so the trace isolates the missing probe rather than also failing BASE-030, and the handshake is left unanswered so no legacy-shaped result has to be judged for `resultType`. |
| `vers-002-retry-with-unsupported-version.jsonl` | After a `-32022` offering `2026-07-28`, the client retries with `1899-01-01` — a version the list it was just handed does not contain (VERS-002) |
| `vers-004-extension-identifier-without-prefix.jsonl` | Client capabilities advertise the extension `ui`: a valid `_meta` key, but extension identifiers require the prefix a `_meta` key may omit (VERS-004) |
| `vers-008-initialize-error-without-versions.jsonl` | A modern-only server refuses a legacy `initialize` with a bare `Method not found`, naming none of the versions it does speak (VERS-008). Necessarily also falsifies BASE-030: a legacy handshake carries no `_meta` envelope — that is what makes it legacy — so the antecedent of this clause is a client that cannot satisfy BASE-030. BASE-031 is *not* also reported, and this trace is why the check was narrowed: `basic/versioning`'s compatibility matrix makes the rejection code implementation-defined for exactly this exchange. |
