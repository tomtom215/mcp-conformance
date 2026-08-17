<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Trace corpus

Fixtures for the golden-corpus tests (`crates/mcp-trace-validator/tests/golden.rs`):

- **`good/`** — sessions that must validate with verdict `pass`.
- **`violations/`** — single-issue sessions, each falsifying at least one check;
  named after the requirement whose check they exist to kill.
- **`draft/{good,violations}/`** — the same two kinds for revision `2026-07-28`,
  judged against that revision's registry behind the `draft-2026-07-28` feature.
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

## Provenance ledger

Every trace's origin, in one reviewable place that survives history rewrites (the
invariant test in `golden.rs` fails if a trace is added without a ledger row). All
current traces share one provenance: **hand-authored for this repository as
synthetic sessions** (no third-party traffic, no recorded production data),
written against the `2025-11-25` spec text fetched live from
modelcontextprotocol.io on 2026-06-09 (re-verified clause-by-clause against the
live text on 2026-06-11) and validated against the embedded registry
at the commit that introduced them. Traces produced by capture tooling (roadmap
M3) will record the capturing implementation and revision here.

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

### `2026-07-28` (`corpus/draft/`)

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
| `stateless-session.jsonl` | Conformant `2026-07-28` stateless session over **stdio**: every request carries its own `_meta` envelope (protocolVersion + clientCapabilities), every result carries `resultType`, request ids are reused only after their response. All 51 checked entries pass, but the Streamable HTTP clauses among them pass by abstention — there is no HTTP framing for them to read, which is why `streamable-http-session.jsonl` exists. |
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
| `disc-001-server-discover-method-not-found.jsonl` | `server/discover` answered with `-32601`, which this revision makes mandatory (DISC-001) |
| `disc-002-dual-era-client-skips-probe.jsonl` | A client that speaks both eras — a modern `_meta` request, then an `initialize` fallback — whose first request is `tools/list` rather than the `server/discover` probe (DISC-002). The `initialize` carries a full `_meta` envelope so the trace isolates the missing probe rather than also failing BASE-030, and the handshake is left unanswered so no legacy-shaped result has to be judged for `resultType`. |
