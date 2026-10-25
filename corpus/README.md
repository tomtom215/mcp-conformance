<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Trace corpus

Fixtures for the golden-corpus tests (`crates/mcp-trace-validator/tests/golden.rs`):

- **`good/`** — sessions that must validate with verdict `pass`, at the revision each
  declares: `validate` with no flags passes all of them but one, the session whose server
  answers `initialize` with `2024-11-05` (`stdio-server-negotiates-2024-11-05`), which `validate`
  refuses by default (exit 2) and passes with `--revision 2025-11-25`. The CI jobs that run
  the shipped Action and image over this directory name that exception.
- **`violations/`** — single-issue sessions, each falsifying at least one check;
  named after the requirement whose check they exist to kill.
- **`draft/{good,violations}/`** — the same two kinds for revision `2026-07-28`,
  judged against that revision's registry.
- **`draft/captured/`** — sessions recorded off the wire from implementations
  this repository did not write. No verdict is asserted for them; their whole
  report is byte-pinned instead. See *Captured versus authored* below.
- **`captured/`** — the `2025-11-25` counterpart: the official SDKs'
  conformance servers recorded by `interop/matrix.sh`, judged against that
  revision's registry, likewise pinned without an asserted verdict. Their
  `2026-07-28` sessions sit in `draft/captured/` beside the others. See *The
  official SDKs' conformance servers* below.
- **`golden/`** — the byte-pinned expected report for every trace, with
  `golden/draft/` holding the `2026-07-28` corpus's. Regenerate only via
  `cargo xtask bless` and review the diff like code.
- **`golden/exclusions/`** — one ledger per revision, holding the clauses that
  revision's registry declines to judge from a trace, and why.

The revisions keep separate golden directories so that where a golden lives
says which revision's registry produced it. Trace names would not collide
without the split: a requirement ID names one clause in one revision and is
never reused ([STABILITY.md](https://github.com/tomtom215/mcp-conformance/blob/main/STABILITY.md)), so `base-045-…` can only
be a `2026-07-28` trace — `BASE-045` is not a `2025-11-25` requirement. A
sentence both revisions carry has an ID in each.

## What a golden pins

A report states two kinds of fact, and they are pinned separately
([ADR-0013](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0013-golden-report-format.md)).

`golden/<stem>.json` holds what the **trace** decided: the revision, the whole
`totals`, and every `pass` / `fail` / `warn` / `not-observed` /
`not-applicable` / `unsupported` row, in registry order.

`golden/exclusions/<revision>.json` holds what the **registry** decided: the
`excluded` rows, whose outcome and prose come from the registry alone and are
identical for every trace judged against it — `engine::build_row` never consults
the session for them. One distinct set per revision, and before the split every
golden restated its revision's whole set: at the time that was 59% of the
corpus's lines.

|  | `2025-11-25` | `2026-07-28` |
|---|---:|---:|
| Goldens | 125 | 238 |
| Excluded rows the ledger holds | 246 | 320 |
| Distinct not-observed sets across them | 86 | 151 |

Every cell is verified against the corpus by `cargo xtask ci`
(`xtask::coverage::corpus`). It is checked rather than trusted because it had
already drifted twice: this paragraph is prose, and prose does not recount
itself.

Nothing stopped being pinned. Splicing a golden back together with its ledger
reproduces the whole report, and `assert_reconstructs_the_full_report` proves it
does on every trace, on every run — so a judged row that went missing, a ledger
that drifted from its registry, or rows that interleave in any order but the
registry's all fail the suite. `totals.excluded` stays in every golden as the
per-trace tie back to the ledger: a clause entering or leaving the excluded set
moves every golden for that revision, one line each.

Only `excluded` collapses. `not-observed` and `not-applicable` are per-trace
evidence, and the table's third row is the measure of it: near one distinct set
per golden, in both revisions. Collapsing those would delete exactly what makes
the captures differentiate each other.

## Violation naming contract

A violation trace is named `area-nnn-<slug>.jsonl` and **must** produce a
Fail/Warn row with findings for exactly requirement `AREA-NNN` — the golden
harness enforces the attribution by name
(`violation_traces_fail_and_match_goldens`, and
`draft::draft_violation_traces_fail_and_match_goldens` for `draft/`), so a
defect re-routed to a different requirement cannot re-bless silently. A stem
that does not begin with a requirement ID fails the suite loudly. The one other
prefix is `wire-schema-<slug>.jsonl`: such a trace must fail the report's wire
schema — the messages against the revision's own JSON Schema, which no
requirement row carries ([ADR-0020](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0020-wire-schema-validation.md)).

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
detail*. A check that starts misfiring on real traffic moves it — which is
precisely the half of the report that stays in the per-trace file.

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
| `http-session.jsonl` | Streamable HTTP session: session-ID assignment and echo, `MCP-Protocol-Version` headers, `Accept`/`Content-Type` discipline, ping (TRAN-011/013/017/018/025/029/039/040 pass paths). It carries all three client request forms, which is what lets the two `Accept` clauses be told apart: POSTs offering both media types (TRAN-025), a standalone-stream `GET` offering `text/event-stream` (TRAN-039), and a session-terminating `DELETE` carrying `Accept: */*`. That last exchange is a regression guard — the clause defining session termination places no `Accept` obligation on it, but until 2026-08-20 both clauses were applied to every client request whatever its method, so this exact teardown drew two MUST-level failures from a client that had violated nothing. |
| `stdio-feature-session.jsonl` | Every feature area conformant in one session: tools (incl. outputSchema + structuredContent, and a call returning an embedded resource against a declared `resources` capability — TOOL-009's pass path), resources (read/blob/subscribe/updated), prompts (text/image/audio/embedded), logging, completion, pagination cursor flow, and a well-formed `params._meta` key (BASE-019/020's pass path) |
| `stdio-client-features.jsonl` | Every client-feature page conformant in one session (added 2026-10-13, review item 2.5): a client declaring `elicitation` in both modes, `sampling.tools` and `roots`; a form elicitation accepted with `content` its `requestedSchema` admits; a URL mode elicitation carrying an `elicitationId` and an RFC 3986 URL, accepted, then completed by `notifications/elicitation/complete` naming that id; a two-round sampling tool loop — `toolChoice` `required` answered with a tool use, then the tool result sent back under `toolChoice` `none` and answered with text; `roots/list` answered with a `file://` root; and a `-32042` listing one URL mode elicitation, completed the same way |
| `stdio-full-session.jsonl` | Handshake plus ping, tools/list, tools/call over stdio |
| `stdio-minimal-init.jsonl` | Smallest conformant session: the three-message handshake |
| `stdio-request-unanswered-at-close.jsonl` | A handshake, then a `tools/list` the server never answers before the transport closes (added 2026-10-24). No clause judges it — JSON-RPC expects every request answered, but `BASE-012` is excluded — so the verdict is `pass`; what the golden pins is the report's `unanswered_requests` note: one request, and that the trace ended with a `transport-close`. Until 2026-10-24 the report said nothing at all. |
| `stdio-requests-partly-unanswered.jsonl` | The same handshake, then three requests of which the server answers one (`tools/list`) and leaves two (`ping`, `tools/call`) pending when the recording simply stops, with no close or abort (added 2026-10-24). The note counts two and says the trace stopped; the verdict is `pass`. |
| `stdio-server-negotiates-2024-11-05.jsonl` | A `2025-11-25` client whose server answers `initialize` with `protocolVersion` `2024-11-05`, a revision this build has no registry for; the client goes on under it (`notifications/initialized`, `tools/list`) and closes (added 2026-10-24). The session ran under what the server named — that is what negotiation means — so `validate` refuses to judge it by default (exit 2, naming both revisions) rather than judging it under the proposal, which until 2026-10-24 it did silently, as a `pass`. This golden is that judgment asked for explicitly: `--revision 2025-11-25` judges the session under the proposal, and it passes, because nothing in it breaks a clause a `2025-11-25` registry can see. |
| `stdio-tool-image-base64-padding.jsonl` | Two `tools/call` results carrying image and audio content whose `data` is well-formed base64 at every padding length — a PNG ending `==`, a WAV and a GIF ending `=`, a JPEG fragment with none — and one image whose `data` is the empty string, which encodes nothing and is well-formed under RFC 4648 §4 (the `format: byte` rule the schema marks `data` and `blob` with accepts it, as `ajv-formats` does; added 2026-10-24). The wire schema passes all five and no clause judges a tool result's encoding, so the verdict is `pass`; the trace exists so that asserting `byte` cannot drift into rejecting padding or emptiness. |
| `stdio-progress.jsonl` | Progress done right over stdio, in both directions (added 2026-10-24, when `basic/utilities/progress` entered the registry): rising values (`0.25`, `0.5`, `1`) for one token, ending before the result; the token reused by a later request once the first was answered, its sequence starting again below where the last one ended; an integer token running concurrently with it; and a server `roots/list` opting into progress under a token of its own, which the client reports on before answering. The pass path of PROG-009/010/011/013/019 |
| `stdio-task-progress.jsonl` | A task-augmented `tools/call` under a server declaring `tasks.requests.tools.call` (added 2026-10-24): answered with a `CreateTaskResult`, then progress under the request's token tagged `io.modelcontextprotocol/related-task`, both before and after a `tasks/get` that still says `working`; a second, ordinary call cancelled by notification while the task exists — CANC-020's pass path, a cancellation that names no task's request — and left unanswered by a server that answers the later `tasks/get`, so it has read the cancellation (CANC-021's pass path); then `tasks/get` reporting `completed`, `tasks/result`, and no progress after. The pass path of PROG-014/015/016 |
| `stdio-cancellation.jsonl` | Cancellation done right over stdio, in both directions (added 2026-10-24, when `basic/utilities/cancellation` entered the registry): a `tools/call` cancelled by the client and left unanswered by a server that answers a later `tools/list`, so demonstrably read it; a second call whose answer crosses its cancellation on the wire — the race the page's Timing Considerations name, which nothing convicts; and a server `roots/list` the server withdraws by `notifications/cancelled`, then a server `ping` the client answers `{}` while leaving the withdrawn request unanswered. The pass path of CANC-018/019/021 and PING-001 from the client's side |
| `http-origin-refused-without-id.jsonl` | A Streamable HTTP `initialize` carrying `Host`/`Origin` `evil.example.com`, refused `403` with a JSON-RPC error that has no `id`, then the same handshake from `localhost` completing. The transport's security warning permits exactly that body ("If the `Origin` header is present and invalid, servers MUST respond with HTTP 403 Forbidden. The HTTP response body MAY comprise a JSON-RPC _error response_ that has no `id`"), so BASE-009 passes. Until 2026-10-13 the framing tied the error to the refused request, whose id was readable, and BASE-009 failed it — the shape of the official suite's `dns-rebinding-protection` scenario against the everything server (`conformance/wire-findings.json`). Added 2026-10-13 |
| `http-authorization.jsonl` | A protected Streamable HTTP server, conformant (added 2026-10-13, review item 2.8): `initialize` refused `401` with a Bearer challenge carrying `resource_metadata` and `scope` (AUTH-014's pass path), then sent again **with the same id**, as the official SDKs' clients retry, and answered; a `ping` refused `403` `insufficient_scope` naming `scope` and `resource_metadata` (AUTH-049/050/051), retried and answered; another refused `401` `invalid_token` (AUTH-045/049) whose `application/json` body is the resource server's own `{"error":"invalid_token",…}` object, retried and answered. That body is the HTTP layer's, not a JSON-RPC message; read as one it failed BASE-006/008/009 and the wire schema. The retries are why a request refused at the HTTP layer is *not delivered* (`context/refusal.rs`): judged as two deliveries, each retry failed BASE-003 and the trace counted two sessions. No metadata fetch appears, because none would: see `checks/auth.rs` for what a recording carries for authorization |

### `violations/`

Each file injects exactly the violation its name states; `golden/` shows every
row the trace decided, including any intrinsic secondary findings the injected
defect causes (a malformed notification also fails lifecycle accounting, for
example).

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
| `page-002-fabricated-cursor-rejected.jsonl` | PAGE-002, and with it PAGE-003's pass path: the client fabricates a cursor and the server answers `-32602` rather than honouring it. The two halves of one event belong to different parties, so the trace falsifies the client's clause and evidences the server's. |
| `prom-001-capability-undeclared.jsonl` | PROM-001 |
| `prom-003-image-data-invalid.jsonl` | PROM-003 (and, since 2026-10-24, the wire schema's `byte` format on the same message) |
| `prom-004-audio-data-invalid.jsonl` | PROM-004 (and, since 2026-10-24, the wire schema's `byte` format on the same message) |
| `prom-005-embedded-resource-malformed.jsonl` | PROM-005 |
| `prom-008-required-argument-unvalidated.jsonl` | PROM-008 |
| `res-001-capability-undeclared.jsonl` | RES-001 |
| `res-004-uri-bad-scheme.jsonl` | RES-004 |
| `res-006-blob-not-base64.jsonl` | RES-006 (and, since 2026-10-24, the wire schema's `byte` format on the same message) |
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
| `tran-018-declined-version-header-refused-400.jsonl` | TRAN-018, a warning: the client asks `initialize` for `2025-06-18`, the server answers `2025-11-25`, and the client's next request carries `MCP-Protocol-Version: 2025-06-18` anyway. The server answers it `400 Bad Request` — TRAN-020's pass path: the server declined that version at `initialize`, so by its own account the header names an unsupported one. Added 2026-10-13 |
| `tran-020-declined-version-header-accepted.jsonl` | TRAN-020 (the same session, the request served `200` rather than refused `400`; TRAN-018 warns for the client's header, as above). Excluded until 2026-10-13 as undecidable — the server's supported set is not in the trace — though the server's own `initialize` answer states one version it does not support. Added 2026-10-13 |
| `tran-025-accept-header-missing.jsonl` | TRAN-025 (a POST with no `Accept` header at all) |
| `tran-025-accept-header-json-missing.jsonl` | TRAN-025 (a POST offering `text/event-stream` only — the half of the clause that read as a pass until the check learned the request method) |
| `tran-039-get-accept-header-missing.jsonl` | TRAN-039 (a standalone-stream GET offering `application/json` only) |
| `tran-049-message-not-posted.jsonl` | TRAN-049 (a `ping` sent by `PUT` after a clean handshake) |
| `tran-026-http-post-batch.jsonl` | TRAN-026 (a batch array POSTed after a clean handshake) |
| `tran-027-notification-answered-200.jsonl` | TRAN-027 (the `notifications/initialized` POST answered `200` instead of `202 Accepted`) |
| `tran-029-content-type-unexpected.jsonl` | TRAN-029 (an `initialize` request answered `200` with `Content-Type: text/html`) |
| `tran-029-error-without-content-type.jsonl` | TRAN-029 (a `tools/list` request answered `500` with a JSON-RPC error and no `Content-Type`). The clause names no status; until 2026-10-13 only success statuses were judged, and the calibration against mcpsnoop (`book/src/mcpsnoop.md`) found the gap. Added 2026-10-13 |
| `tran-040-get-answered-json.jsonl` | TRAN-040 (a standalone-stream GET answered `200` with `application/json` rather than `text/event-stream` or `405`) |
| `base-017-input-schema-invalid.jsonl` | BASE-017 (a tool `inputSchema` whose property declares `"type": "text"`, not a JSON Schema type — invalid in 2020-12, the default dialect) |
| `base-086-input-schema-draft-07.jsonl` | BASE-086, a warning (a tool `inputSchema` declaring `$schema` draft-07: valid in its dialect, so BASE-017 passes it, and permitted, but not the 2020-12 that "Implementors are RECOMMENDED to use") |
| `tool-011-structured-content-not-conforming.jsonl` | TOOL-011 (`structuredContent` of the right shape, an object, whose `temperature` is the string `"warm"` where the tool's `outputSchema` says `number` — passed until the check could apply the schema) |
| `wire-schema-resource-missing-uri.jsonl` | Wire schema: a `resources/list` result whose resource has no `uri`, which `Resource` requires |
| `wire-schema-log-level-not-in-enum.jsonl` | Wire schema: a `notifications/message` at level `chatty`, outside `LoggingLevel` |
| `wire-schema-content-block-unknown-type.jsonl` | Wire schema: a `tools/call` result whose content block has `type: "txt"`, matching no `ContentBlock` member |
| `wire-schema-tool-image-data-not-base64.jsonl` | Wire schema: a `tools/call` result whose image `data` is `not base64!`. `ImageContent.data` carries `format: byte` in the specification's schema, which the validator asserts since 2026-10-24; no clause of this revision judges a tool result's encoding (the prompt-content and resource-blob checks read their own members), so until then this message passed the wire schema and drew nothing at all. The official suite does not catch it either: it loads `ajv-formats` and then registers `byte` as always valid (`addFormat("byte", true)`). |
| `elic-004-answered-without-capability.jsonl` | ELIC-004: a client declaring only `sampling` answers a form elicitation with a result. Also LIFE-009 and ELIC-006 for the server's request — a client can only show support it never declared by answering something it should not have been sent |
| `elic-006-url-mode-undeclared.jsonl` | ELIC-006: a URL mode elicitation sent to a client declaring `elicitation: {}`, which is form mode only (LIFE-009 reads the same sub-capability). The client refuses with `-32602` — ELIC-021's pass path |
| `elic-007-request-without-message.jsonl` | ELIC-007: a form elicitation with no `message`. The wire schema rejects the same request |
| `elic-009-form-without-schema.jsonl` | ELIC-009: a form elicitation with no `requestedSchema`; the wire schema too |
| `elic-011-url-without-elicitation-id.jsonl` | ELIC-011: a URL mode elicitation with no `elicitationId`, which this revision's table requires; the wire schema too |
| `elic-012-url-without-scheme.jsonl` | ELIC-012: a URL mode `url` of `mcp.example.com/connect?…`, with no scheme. The schema marks `url` `format: uri`, which ADR-0020 leaves unasserted, so the clause is the only thing that catches it |
| `elic-014-complete-names-unknown-id.jsonl` | ELIC-014: a completion notification naming `e-99` after the session's only URL elicitation established `e-42` |
| `elic-018-url-required-error-without-list.jsonl` | ELIC-018: a `-32042` whose `data` lists no elicitations; the wire schema too |
| `elic-019-url-required-error-form-mode.jsonl` | ELIC-019: a `-32042` listing a form mode elicitation; the wire schema too |
| `elic-021-undeclared-mode-accepted.jsonl` | ELIC-021: a client declaring form mode only accepts a URL mode request instead of refusing it with `-32602`. ELIC-006 and LIFE-009 for the server's request, the clause's antecedent |
| `elic-049-accepted-content-violates-schema.jsonl` | ELIC-049, a warning: `accept` with `{"city": 75001}` against a schema typing `city` as a string |
| `samp-004-tools-without-sampling-tools.jsonl` | SAMP-004: `tools` sent to a client declaring plain `sampling` (LIFE-009 reads the same sub-capability) |
| `samp-005-answered-without-capability.jsonl` | SAMP-005: a client declaring only `elicitation` answers a sampling request with a result. LIFE-009 for the server's request, the antecedent |
| `samp-006-include-context-deprecated.jsonl` | SAMP-006, a warning: `includeContext` `thisServer` to a client that declares `sampling.context` — SAMP-007's pass path, since the declaration permits what SAMP-006 still asks the server to avoid |
| `samp-007-include-context-undeclared.jsonl` | SAMP-007, a warning: `includeContext` `allServers` without `sampling.context`. SAMP-006 warns too: any use of the value is one it should have avoided |
| `samp-008-tool-results-mixed.jsonl` | SAMP-008: a user message mixing text with a tool result. Also SAMP-009 — the message after the tool use is not one of tool results only — and SAMP-019, which restates both. The client refuses with `-32602`, SAMP-012's pass path |
| `samp-009-tool-use-unanswered.jsonl` | SAMP-009: a tool use followed by a plain text message (SAMP-019 restates it); the client refuses with `-32602` |
| `samp-010-required-tool-choice-ignored.jsonl` | SAMP-010: `toolChoice` `required` answered with text and no tool use |
| `samp-011-none-tool-choice-ignored.jsonl` | SAMP-011: `toolChoice` `none` answered with a tool use |
| `samp-012-malformed-tool-messages-answered.jsonl` | SAMP-012, a warning: the client answers, with a result, a request whose last message is an unanswered tool use. SAMP-009 and SAMP-019 for the request, the antecedent |
| `root-001-answered-without-capability.jsonl` | ROOT-001: a client declaring only `sampling` answers `roots/list` with a root. LIFE-009 and ROOT-009 for the server's request (the antecedent), and ROOT-004 warns: having declared no roots, the client should have refused with `-32601` |
| `root-003-root-not-file-uri.jsonl` | ROOT-003: a root listed as `https://example.com/repos/trips` |
| `root-004-unsupported-answered-internal-error.jsonl` | ROOT-004, a warning: a client without `roots` refuses `roots/list` with `-32603` rather than `-32601`. LIFE-009 and ROOT-009 for the request, the antecedent |
| `root-009-asked-without-capability.jsonl` | ROOT-009, a warning: `roots/list` sent to a client that did not declare `roots` (LIFE-009 states the MUST behind it). The client refuses with `-32601` — ROOT-004's pass path |
| `auth-014-challenge-without-scope.jsonl` | AUTH-014, a warning: the `401` challenge carries `resource_metadata` but no `scope` |
| `auth-045-invalid-token-answered-403.jsonl` | AUTH-045: a challenge declaring `error="invalid_token"` sent with `403`. AUTH-049 fails too: the error-handling table gives the same error the same status |
| `auth-049-insufficient-scope-answered-401.jsonl` | AUTH-049: `error="insufficient_scope"` sent with `401` rather than the table's `403`. AUTH-050 warns too: its list begins with the `403` |
| `auth-050-insufficient-scope-without-resource-metadata.jsonl` | AUTH-050, a warning: a `403` `insufficient_scope` challenge naming its scopes but no `resource_metadata` |
| `auth-051-insufficient-scope-names-no-scope.jsonl` | AUTH-051, a warning: a `403` `insufficient_scope` challenge with no `scope`. AUTH-050 warns too: `scope` is one of the parameters its list requires |
| `ping-001-ping-answered-with-content.jsonl` | PING-001: a `ping` answered with `{"pong":true,"uptime":4212}` rather than an empty result. Added 2026-10-24, when `basic/utilities/{ping,progress,cancellation}` entered the registry; the rows below to `canc-021` came with it |
| `prog-009-progress-token-float.jsonl` | PROG-009: a request opting into progress with `progressToken: 1.5`, a number but not an integer. The wire schema fails the request and the notification that echoes the token too |
| `prog-010-progress-token-reused-in-flight.jsonl` | PROG-010: two `tools/call` requests in flight at once under the same progress token, so a notification could not say which it reports on. Reuse after an answer is legal, and `good/stdio-progress.jsonl` does it |
| `prog-011-progress-value-decreases.jsonl` | PROG-011: progress `50` then `10` for one token |
| `prog-013-progress-token-never-issued.jsonl` | PROG-013: a progress notification for the token `"ghost"`, which no request in the session provided — the session begins with `initialize` in the recording, so every request it issued is there, and none carried it. The shape the dogfood of 2026-10-24 found judged clean at this revision |
| `prog-019-progress-after-completion.jsonl` | PROG-019: a progress notification for a stdio request after its result (PROG-013 — the token no longer names an in-progress operation — and PROG-016 share the check) |
| `prog-015-task-progress-token-differs.jsonl` | PROG-015: a task created for a request carrying `progressToken: "deploy-1"` reports progress tagged with the task's id under `"task-1-progress"` (PROG-014 states the same rule and shares the check). PROG-013 fails too: that token was never provided by any request |
| `prog-016-task-progress-after-terminal-status.jsonl` | PROG-016: progress for a task after a `tasks/get` reported it `completed` (PROG-013 and PROG-019 share the check: the task is the request's operation, and it is over) |
| `canc-018-cancellation-of-unknown-request.jsonl` | CANC-018: the client cancels request `99`, which it never issued in a session whose `initialize` the recording holds. The shape the dogfood of 2026-10-24 found judged clean at this revision |
| `canc-019-initialize-cancelled.jsonl` | CANC-019: the client cancels its own `initialize` request before the server answers it |
| `canc-020-task-cancelled-by-notification.jsonl` | CANC-020: `notifications/cancelled` naming the `tools/call` a `CreateTaskResult` answered; a task is cancelled with `tasks/cancel` |
| `canc-021-response-after-cancellation-read.jsonl` | CANC-021, a warning: the server answers a request the client cancelled by notification — *after* answering a `tools/list` the client sent later than the cancellation, which on stdio's ordered stream proves it had read the cancellation. The answer that merely crosses a cancellation is the race `good/stdio-cancellation.jsonl` carries, and is not reported |

### `2026-07-28` captured (`corpus/draft/captured/`)

Two recordings of **the same client driving the same scenarios**, differing in
one variable: which revision the server was serving. That is what makes the pair
worth committing — a single capture shows what one implementation does, while a
matched pair shows what the *revision* costs, and every difference between the
two reports is attributable to that one change.

| Field | `official-suite-2026-07-28-scenarios.jsonl` | `official-suite-2026-07-28-stateless.jsonl` |
|---|---|---|
| Client | The **official MCP conformance suite**, `0.2.0-alpha.12`, driving its `2026-07-28` scenario set (the pin `cargo xtask draft-readiness` holds) | The same client, the same scenarios, the same run |
| Server | `mcp-everything-server` on rmcp 3.5.1, serving **`2025-11-25`** — held to a revision it does not implement, so genuine non-conformance is the expected content | `mcp-everything-server --protocol-version 2026-07-28`, its stateless mode, same build |
| Recorded by | `mcp-everything-server`'s tap, during `cargo xtask draft-readiness`, 2026-10-13 | same run, second leg |
| Contents | 99 events / 24 POST exchanges | 99 events / 24 POST exchanges |
| Our verdict | 70 pass, 0 fail, **1 warn**, 110 not observed, 320 excluded | **71 pass, 0 fail, 0 warn**, 110 not observed, 320 excluded |
| The official runner's verdict, that run | `42 passing / 0 failing` | `42 passing / 0 failing` |

Both carry `server/discover`, `tools/list`, `tools/call`, `completion/complete`,
`resources/{list,read}`, `prompts/{list,get}` and progress notifications; every
request carries the revision's per-request `_meta` envelope and its SEP-2243
metadata headers, and neither carries an `initialize` anywhere. Their
`tools/list` results differ by two entries, which is the point of recording both:
`test_url_elicitation` and `test_list_changed` are listed by the legacy server
(18 tools) and not by the stateless one (16), because what each tool does has no
place at `2026-07-28`: that revision removed `notifications/elicitation/complete`
and URL mode's `elicitationId` (URL-mode elicitation itself remains), and moved
`list_changed` notifications into subscriptions. Each listing is the first page;
neither session follows `nextCursor`.

**The one finding left: BASE-037, a warning, on the legacy leg.** Of that
server's 24 results only the `server/discover` one carries
`io.modelcontextprotocol/serverInfo` in its `_meta`; rmcp sets it there and on
`subscriptions/listen` and nowhere else. The stateless server names itself on all
24, as it has since 2026-10-13. The legacy server is left without the field on
purpose: it implements `2025-11-25`, which names the server once, in
`initialize`, and the measurement is that server held to a revision it does not
implement (`ServedRevision::names_itself_on_results`). It is a SHOULD that
exempts a server "specifically configured not to", so it warns and never fails a
run. The same warning stands on every other rmcp server that does not add the
field itself (`sdk-rust-2026-07-28`); the Go and Python SDK servers set it on every
result (`sdk-go-2026-07-28`, `sdk-python-2026-07-28`).

**Two findings that used to be here were ours, not the implementations'.** An
earlier version of this pair reported TRAN-058 (client POSTs missing
`Mcp-Method`/`Mcp-Name`) and TRAN-068 (SSE responses missing
`X-Accel-Buffering: no`). Both were artifacts of the tap's recording allowlist,
which predated SEP-2243 and did not record those headers — the client sent them
(rmcp's transport rejects a `2026-07-28` request without `Mcp-Method` before
dispatch, and the suite scored 23/23) and the server sent them (rmcp sets
`X-Accel-Buffering` on every SSE response it builds). The allowlist was
extended; both clauses now pass on both captures. The lesson is recorded rather
than quietly fixed: **a check can only be as honest as the recording it reads,
and a capture path that silently drops evidence manufactures findings against
conforming implementations.**

**The same lesson, a third time, one field over (2026-08-20).** The tap recorded
an exchange's headers but not its *method*, so `TRAN-025` and `TRAN-039` — the
`Accept` obligations of a POST and of a GET — were enforced as one rule applied
to every client request whatever its form. That was wrong twice over, and both
halves were found by pointing this workspace's own reference host at its own
reference server over HTTP and validating the tap: the conforming
session-teardown `DELETE`, which owes no `Accept` header at all, drew two
MUST-level failures, while a POST omitting `application/json` — the verbatim
violation `TRAN-025` names — was reported `pass`, because the one shared rule
could only demand what both clauses had in common. `EventBody::Http` now carries
the method, each clause is judged against the requests it binds, and an exchange
whose method the capture did not record is judged by neither. These two captures
were re-recorded on 2026-08-20 so the method is present, on 2026-10-04 when the
draft ratchet moved to `alpha.12`, and on 2026-10-13 (below).

**Why the runner's 23/23 and our 59-vs-60 were both right — and how the runner
caught up.** The suite's `2026-07-28` scenarios exercise features — list a
thing, call a thing, read a thing — and a `2025-11-25` server answers all of
them, because rmcp serves a per-request-versioned POST whichever revision the
handler advertises. At `0.2.0-alpha.9` that was the whole of the runner's
reading, so it scored both servers 23/23. The registry here judges the
specification's prose instead, so it saw the one place the two servers actually
differ: CACH-001, no `ttlMs` on cacheable results.

**`0.2.0-alpha.11` found the same thing, six weeks later.** Its new
`wire-schema-valid` check validates every message against the negotiated
revision's JSON schema, and it fails the `2025-11-25` leg's four resource
scenarios for `must have required property 'cacheScope'` / `'ttlMs'` — that
clause, independently. Measured at `alpha.11`, the legacy leg scored `37 passing / 4
failing` and the stateless leg `41 / 0`, so the runner separated the two
servers it could not tell apart before. Neither instrument was wrong; they were
measuring different things, and one of them saw this first. The pair is still
the evidence for taking both readings — now with a worked example of the
prose-level reading arriving earlier than the schema-level one.

**And then the implementation closed the gap.** rmcp 3.5.1 fills in missing
cache hints for any peer speaking `2026-07-28` (rust-sdk #1308), so since the
workspace moved to it (2026-10-12) the legacy server's results carry `ttlMs` and
`cacheScope` too (`ttlMs: 0` on `resources/list`, rmcp's default rather than
this server's choice), and `cargo xtask draft-readiness` scores both legs 42
passing / 0 failing. The pair was re-recorded on 2026-10-13, so the legacy leg
no longer fails CACH-001 or the wire schema; until then it was kept as recorded
on 2026-10-04 (69 pass, CACH-001 failing; the runner `38 passing / 4 failing`),
and this section's history above is the evidence for what each instrument caught
and when. With the hints filled in, the runner again scores both servers alike,
and only the registry's BASE-037 warning tells them apart.

The 110 not-observed rows are the honest denominator: of the 181 clauses this
revision's registry can judge, these sessions carried subject matter for 71.
They open no subscription, present no cursor, draw no JSON-RPC error, and send
no malformed `_meta`, so those clauses are neither passed nor failed here — they
are untested, and the report says which.

Regenerate both with `cargo xtask draft-readiness` and copy
`target/draft-readiness/<revision>/tap/001-stateless.jsonl`; the ephemeral port
in the `host` header changes per run, so re-copying is a deliberate act with a
golden diff, not something to do casually.

#### The stdio capture

| Field | `reference-host-2026-07-28-stdio.jsonl` |
|---|---|
| Client | This workspace's `mcp-reference-host`, on rmcp's stateless client lifecycle |
| Server | `mcp-everything-server --transport stdio --protocol-version 2026-07-28` |
| Recorded by | The host's own `--trace-dir` capture, during `cargo xtask draft-capture`, 2026-10-13 |
| Contents | `server/discover`, a full `subscriptions/listen` lifecycle, an 18-tool sweep with four MRTR rounds (three elicitations and one sampling), and a discovery-driven sweep of everything that is not a tool: `resources/{list,templates/list,read}`, `prompts/{list,get}` for all five prompts, `completion/complete`, and one read of a URI the catalog does not contain. Every listing is followed to its last page: the server serves each in two, the suite's catalogue first and the rest — icons, annotations, a `resource_link` result, an audio prompt, `x-mcp-header` designations — behind a `nextCursor`. Every call carries a W3C `traceparent` in its `_meta` (BASE-040), and the session closes with a `notifications/cancelled` naming a request the server had already answered, then one more call the server *may* answer — the only shape a recording can take for a MUST NOT (TRAN-123/TRAN-124) |
| Our verdict | **109 pass, 0 fail, 0 warn**, 72 not observed, 320 excluded |

**It is the only capture that exercises `subscriptions/listen`.** The official
suite drives no subscription, so the four judged `SUBS` clauses — and BASE-039,
which binds a subscription stream's notifications — are backed by this file
alone: an acknowledgment, a filter the server narrowed, four tagged
notifications and an empty graceful-closure result. The same holds for the MRTR
clauses. In the two HTTP captures every one of those rows now reads **not
observed**; here they read `pass`.

**Two defects were found by enriching it, and neither could have been found any other way.** Until the session read a resource the catalog does not have, nothing in this workspace had ever exercised the server's not-found path at this revision — and it answered `-32002`, which `2026-07-28` withdraws outright (`basic/index#error-codes` lists it under "Implementations of this protocol version **MUST NOT** emit these codes" and names `-32602` as the replacement). The second was ours: the host's recorder assigned `seq` when the *send future* completed rather than when the message was handed to the transport, so a reply received while that future was still pending landed in the trace ahead of its own request. That reads as the server answering an id nobody asked, and it failed `BASE-046` on roughly one capture in three. Both are fixed, both have regression tests, and both are the argument for driving more traffic rather than more fixtures.

That asymmetry is the reason to keep all three, and it used to be invisible.
Until the vacuous-pass fix, those rows read identically across the three files
— `pass` everywhere, whether or not the session had opened a stream — and this
paragraph recorded that as a known reporting limitation. It is a limitation no
longer: every check counts the subjects it examined, so a clause with nothing
to look at reports `not-observed`. Read in the other direction, the captures
are complementary rather than redundant: the HTTP pair evidences the transport
header and status clauses, which no stdio recording can carry, and this one
evidences the subscription, MRTR, prompts, resources, logging, completion and
error-code clauses, which theirs never reach.

**What the 48 not-observed rows still are, and why.** Thirty-three are
Streamable HTTP mechanisms a stdio recording structurally cannot carry:
twenty-five clauses of the `TRAN-056`…`TRAN-102` band (it holds 27 judged
clauses, and the two a stdio session does reach, `TRAN-060` and `TRAN-066`,
are judged here), the seven rows of the `x-mcp-header` behaviour table
(`TRAN-130`…`TRAN-136`), and `TOOL-034` — header mirroring is an HTTP
mechanism, and "Clients using other transports (e.g., stdio) MAY ignore
`x-mcp-header` annotations entirely". Eight are server *rejection* rules —
`BASE-031`, `BASE-032`, `BASE-035`, `BASE-036`, `VERS-001`, `VERS-002`,
`VERS-008`, `LOG-010` — reachable only by a client that deliberately sends
something malformed, which this session does not: it is the conforming
capture, and a probe session is a separate recording with a separate expected
report. Two, `CANC-003`/`CANC-011`, bind a server that sends
`notifications/cancelled`, and this one ends every subscription gracefully
instead. `TOOL-022` is the interesting one: rmcp's client caches `tools/list`
under the server's own `ttlMs`, so a second listing never reaches the wire — a
conforming client cannot exercise the deterministic-order clause within the
TTL, which is a property of the caching feature working rather than a gap to
close (the official suite's runner does not cache, and judges it on both of
its captures). The remaining four are not driven by this host: `TRAN-128` and
`DISC-002`'s dual-era probe, `MRTR-024`, and `VERS-004`.

**The client-feature pages added twelve more on 2026-10-13, on both legs**
(review item 2.5). The session's one sampling round carries no `tools`, so
nothing it sends exercises the tool-use clauses (`SAMP-026`, `SAMP-030`…`SAMP-033`,
`SAMP-042`) or the deprecated `includeContext` permission (`SAMP-029`); it asks
for no roots (`ROOT-013`, `ROOT-014`, `ROOT-018`), and none of its elicitations
is in URL mode (`ELIC-064`, `ELIC-065`). The three form elicitations and the
sampling round are judged. When they were first judged one of them warned:
**`ELIC-095` ("Clients SHOULD validate all responses against the provided
schema") on the host**, whose `AcceptWithDefaults` policy answered the `probe`
form — `username` and `email` required, neither with a default — with `accept`
and empty `content`. That was a finding against this project's own client, on
both legs, not a reading the text leaves open, and it was fixed the same day:
the policy now fills each required field without a default with a
deterministic value of its type (`"scripted"` for both of these plain strings),
and declines a form whose required fields no value can satisfy rather than
accepting it with content the schema rejects
(`mcp_reference_host::script::accepted_content` states the rule). Both legs
were re-recorded with the fix and pass it.

**Five used to be on that list for want of server surface; four have left
it.** `CACH-015`/`CACH-016` needed a catalogue served in pages and `PROM-017`
a prompt carrying audio, and both are on the listings' second pages now;
`TOOL-033` needed an `x-mcp-header` designation, and judges it as listed,
which any transport carries (`TOOL-045` passes beside it). The fifth,
`TOOL-034`, is in the HTTP group above: the designation exists now, and only
the transport keeps it unobserved here.

#### The HTTP capture of the same session

| Field | `reference-host-2026-07-28-http.jsonl` |
|---|---|
| Client | The same `mcp-reference-host`, driving the identical session over Streamable HTTP |
| Server | `mcp-everything-server --transport http --protocol-version 2026-07-28` |
| Recorded by | **The server's tap**, during `cargo xtask draft-capture`, 2026-10-13 |
| Contents | 195 events — the session's 103 messages plus 92 `http` events carrying status and headers, the `Mcp-Param-Region`/`Mcp-Param-Limit` pair rmcp mirrors for the designated tool among them |
| Our verdict | **125 pass, 0 fail, 0 warn**, 56 not observed, 320 excluded |

**Recorded by the server, not the host, and that is the whole point.** The
host's recorder sits at rmcp's `Transport` seam, which carries protocol
messages and nothing else — redaction by construction, and no HTTP framing to
record even if it wanted to. So a host-side HTTP recording would report the
Streamable HTTP clauses as *not observed* exactly like the stdio
one. The server's tap sits above the transport and sees the status line and
the conformance-relevant headers, which is why this leg is the only recording
in the corpus that can bear on them at all. Same session, both ends, one file
each: the difference between the two reports is attributable to the transport
and to nothing else.

At 125 of the 181 judgeable clauses it is the best-covered capture here, and
the fourteen it gained on 2026-10-13 are the server's own doing: the second
pages carry the pagination clauses (`CACH-015`/`CACH-016`), the audio prompt
(`PROM-017`), and a tool designating three arguments for `Mcp-Param-*`
mirroring, which rmcp's client mirrors when the tool loop calls it — two
supplied and mirrored, one left out and not, which is the header table's
"value provided" and "not in arguments" rows from both sides (`TOOL-033`,
`TOOL-034`, `TOOL-045`, `TRAN-079`…`TRAN-082`, `TRAN-130`, `TRAN-134`,
`TRAN-135`, `TRAN-137`). Its 33 not-observed rows are the fourteen rejection
rules only the probe exercises, seven clauses judged on the stdio leg and not
on this one (`CANC-005`, `PROG-005`, `PROG-008`, `TRAN-117`, `TRAN-118`,
`TRAN-123`, `TRAN-124`), `TOOL-022` (rmcp's client caches `tools/list` under
the server's own `ttlMs`, so a second listing never reaches the wire),
`VERS-004`, which only the Rust SDK's capture evidences, and the ten clauses
no capture reaches, each with its reason under *What the captures evidence,
together*.

#### The probe session

| Field | `probe-2026-07-28-http.jsonl` |
|---|---|
| Client | `mcp-reference-host --probe`: ten hand-built HTTP requests, each wrong on purpose, and the two conforming `tools/list` pages the last of them is built from |
| Server | `mcp-everything-server --transport http --protocol-version 2026-07-28` |
| Recorded by | The server's tap, during `cargo xtask draft-capture`, 2026-10-13 |
| Contents | A `_meta` envelope missing a required field; an unimplemented protocol version and the retry after it; a header/body version mismatch; an unknown method; a log level outside RFC 5424's eight; a fabricated cursor; a tool needing a capability the request never declared; the removed `initialize` handshake; and — after listing the tools, following `nextCursor` — a call of a tool with a designated argument whose `Mcp-Param-*` header is left out |
| Our verdict | Judged against [`conformance/probe-baseline.json`](https://github.com/tomtom215/mcp-conformance/blob/main/conformance/probe-baseline.json), not for cleanliness |

**A conforming client cannot exercise a rejection rule.** The clauses of
this revision that say what a server owes a request it must *not* serve all
reported *not observed* on every recording here, because nothing
had ever sent such a request. This file is that request, ten times over.

The probes are built outside rmcp, and that is the point rather than a
shortcut: rmcp's client is what makes the other captures trustworthy — it
builds the `_meta` envelope, mirrors the SEP-2243 headers, and will not emit an
ill-formed request — so it is structurally incapable of being the probe. The
bytes here are the fixture.

**Its verdict is a ledger, not a pass.** The probe breaks client-side clauses
by construction: `BASE-030` because its first request omits a required `_meta`
field, `TRAN-071`/`TRAN-072` because two probes are about exactly those
headers, `TRAN-079`/`TRAN-130` because the last one leaves out the header
whose absence `TRAN-136` obliges the server to reject. Demanding a clean report would mean demanding a probe that probes nothing. So
every finding is listed in `conformance/probe-baseline.json` with a reason, and
the gate holds the set in both directions: a finding not in the ledger is a new
defect or a regression, and a listed finding that stopped occurring is either a
fix that should retire its entry or a check that quietly stopped firing.

**It has already worked in both directions.** The first probe run drew two
server-side findings — the server served a request naming log level
`"chatty"`, and honoured a cursor it never issued — which went into the ledger
as open defects. When they were fixed, the gate refused the change until their
entries were retired, which is the half of a ratchet that is easy to leave
out. A third went the same way on 2026-10-13: BASE-037 entered the ledger as
an open server-side warning the day the clause became judged, and its entry
was retired in the change that put `serverInfo` on every result. Every rejection clause the probe reaches now passes; fourteen clauses pass
on this capture and on no other (`BASE-031`, `BASE-032`, `BASE-035`,
`BASE-036`, `LOG-010`, `TRAN-073`, `TRAN-074`, `TRAN-075`, `TRAN-098`,
`TRAN-102`, `TRAN-136`, `VERS-001`, `VERS-002`, `VERS-008`). The fabricated cursor is still
sent, and judged by nothing: on 2026-10-04 `PAGE-010` and `PAGE-011` were
excluded at this revision, because its caching rules make a cursor issued
before the recording began a valid one (the registry entries record the
reasoning). The server still refuses it, and
`a_fabricated_cursor_is_refused_on_the_stateless_revision` in
`mcp-everything-server`'s tests holds it to that.

**Its provenance is weaker than the pair above, and deliberately labelled so.**
Both ends of this session are ours: the official suite drives servers over
`--url` only, so there is no third-party client for stdio to be recorded
against. What it still supplies that an authored fixture cannot is that neither
end was written to satisfy these checks, and that the machinery producing every
byte — the stateless lifecycle, the `_meta` envelope, the MRTR retry loop — is
rmcp's, not this repository's.

It is also the only capture that judges a *verdict* rather than pinning a
report. `cargo xtask draft-capture` fails on any finding, because a finding in
a session where both ends are ours is a defect here rather than news about
somebody else's code. Regenerate with `BLESS=1 cargo xtask draft-capture`
(which rewrites this file) followed by `cargo xtask bless`.

**That gate was red for part of 2026-10-13, on two warnings, and is clean
again.** BASE-037 became judged that day, and this server — rmcp's result
types — named itself in `_meta.io.modelcontextprotocol/serverInfo` only on its
`server/discover` and `subscriptions/listen` results; the client-feature pages
added ELIC-095 against the host (above). The gate was left red rather than
loosened until both were fixed here: the everything server now puts
`serverInfo` on every result it builds at `2026-07-28` (`mcp_everything_server`'s
private `server::identity` module, applied at each handler's return beside the
caching hints), and the host's form answers satisfy their schema. Both
reference-host captures and the probe were re-recorded with the fixes, and the
probe baseline's server-side BASE-037 entry was retired in the same change.

**What it caught.** The first recording of this session reported six failures
the HTTP captures could not: TRAN-060/066/119/120 and MRTR-001, because the
interactive tools still sent `elicitation/create` and `sampling/createMessage`
as independent server-to-client requests — the mechanism SEP-2322 replaced —
and LOG-008, because the logging tool emitted `notifications/message` for a
request that had not asked for them. The official suite's `2026-07-28`
scenarios exercise no interactive tool and no logging tool, so an HTTP-only
corpus would have shipped both defects. That is the argument for capturing both
transports rather than treating one as representative.

#### What the captures evidence, together

The table below is generated from the committed golden reports by
`cargo xtask draft-coverage` and verified by `cargo xtask ci` — per ADR-0001
these numbers are a projection of the data, not prose anyone maintains. The
same gate parses every count stated elsewhere in the living Markdown — the "N
of the M judgeable clauses" claims and the "N pass, M fail" verdicts alike —
and fails when one disagrees, which is how the counts in this section stopped
being wrong. A number quoted rather than asserted goes in backticks, and is
read as a specimen instead.

<!-- draft-coverage:begin (generated by `cargo xtask draft-coverage`; do not edit by hand) -->
| Capture | Judged | pass | fail | warn | Not observed |
|---------|-------:|-----:|-----:|-----:|-------------:|
| `official-suite-2026-07-28-scenarios` | 71 | 70 | 0 | 1 | 110 |
| `official-suite-2026-07-28-stateless` | 71 | 71 | 0 | 0 | 110 |
| `probe-2026-07-28-http` | 85 | 73 | 11 | 1 | 96 |
| `reference-host-2026-07-28-http` | 125 | 125 | 0 | 0 | 56 |
| `reference-host-2026-07-28-stdio` | 109 | 109 | 0 | 0 | 72 |
| `sdk-go-2026-07-28` | 83 | 78 | 4 | 1 | 98 |
| `sdk-python-2026-07-28` | 65 | 65 | 0 | 0 | 116 |
| `sdk-rust-2026-07-28` | 74 | 73 | 0 | 1 | 107 |
| `sdk-typescript-v2-2026-07-28` | 72 | 72 | 0 | 0 | 109 |
| **Union** | **148** | | | | **33** |

Across all 9 captures, **148 of the 181 judgeable clauses** are evidenced by at least one recording. Each capture's own judged count is what *that* recording carried subject matter for; everything else it reports *not observed* rather than counting it as a pass.

The 33 clauses no capture reaches: `AUTH-097`, `AUTH-124`, `AUTH-129`, `AUTH-130`, `AUTH-131`, `AUTH-132`, `AUTH-133`, `CANC-003`, `CANC-011`, `DISC-002`, `ELIC-064`, `ELIC-065`, `MRTR-024`, `ROOT-013`, `ROOT-014`, `ROOT-018`, `SAMP-026`, `SAMP-029`, `SAMP-030`, `SAMP-031`, `SAMP-032`, `SAMP-033`, `SAMP-042`, `TOOL-035`, `TOOL-046`, `TRAN-061`, `TRAN-070`, `TRAN-083`, `TRAN-084`, `TRAN-096`, `TRAN-128`, `TRAN-132`, `TRAN-133`.
<!-- draft-coverage:end -->

The probe session closed the largest group — the rejection rules — and the
cancellation round closed the next. The official SDKs' conformance servers
(below) closed six more without a line of this repository changing: each of
the three that serve this revision lists a tool carrying an `x-mcp-header`
designation, which `TOOL-033`, `TOOL-045`, `TRAN-080`, `TRAN-082` and
`TRAN-137` judge, and the Rust SDK's declares an extension, which `VERS-004`
judges.

**The reference server's own surface closed ten more on 2026-10-13.** Every
listing is now served in two pages — the suite's catalogue first, then what
the suite does not name — and both captures of the host follow `nextCursor`:
the pages are `CACH-015`/`CACH-016`'s subject, and the second prompt page
carries audio for `PROM-017`. A second-page tool designates three arguments
for `Mcp-Param-*` mirroring, and the host's tool loop calls it over HTTP with
two supplied and one left out, so rmcp mirrors two headers and omits the third
(`TOOL-034`, `TRAN-079`, `TRAN-081`, `TRAN-130`, `TRAN-134`, `TRAN-135`). The
probe then sends the same call with one header missing, which the server must
refuse and does (`TRAN-136`).

What is left — 33 clauses — is not backlog in the ordinary sense; each stays
for a reason a richer session would not remove. Ten are the transport and
lifecycle clauses below; the other 23 came with the client-feature and
authorization pages and are grouped after them.

**Two are out of an rmcp client's reach**: `TRAN-132`/`TRAN-133`, the
behaviour table's "Parameter value is `null`" row. A client can only send
`null` for a designated argument whose schema admits it, and rmcp's client
drops any tool whose designated property's `type` is not a single primitive —
`["string", "null"]` included — so a nullable designation never reaches an
rmcp client's call, and this host synthesizes only required arguments and
never a `null`. The row's pass path is authored
(`good/http-x-mcp-header-omitted.jsonl`), and the rmcp restriction is a
candidate to raise upstream.

**One cannot be recorded by the server's tap**: `TRAN-096`, a recognized
`Mcp-Param-…` header holding invalid characters. HTTP parsing strips a value's
edge whitespace before any handler sees it, and the tap records only values
that decode as visible ASCII — rmcp likewise reads anything else as no header
at all, and refuses the call as a missing one — so the antecedent never
reaches the recording however the request is built. Authored traces carry
both halves (`draft/violations/tran-096-invalid-param-header-accepted.jsonl`,
`draft/violations/tran-077-param-header-value-rejected.jsonl`).

**Three belong to a client no capture has**: `TRAN-061` needs a client to POST
a *notification* over Streamable HTTP, and no `2026-07-28` capture contains one
— rmcp's stateless client cancels by closing the response stream, and neither
the official runner nor the SDK client sends any. `DISC-002` and `TRAN-128`
need a dual-era client that probes before falling back to `initialize`, and
this host speaks only the stateless revision.

**Two need server behaviour this one does not have**: `CANC-003`/`CANC-011`
bind a server that sends `notifications/cancelled`, which at this revision may
only tear down a `subscriptions/listen` stream. This server ends every
subscription gracefully instead — the closure `SUBS-005`/`SUBS-006` judge, and
pass — so tearing one down by cancellation would be a second closure path,
added to light up a check; the pass path is authored
(`good/subscription-server-teardown.jsonl`).

**One cannot be reached by a conforming host at all**, and saying so is more
useful than closing it. `MRTR-024` is a server *re-asking* after a client's
shortfall, and a retry that omits requested input is the client violation
`MRTR-015` names: driving it would make the reference host non-conforming,
which is the one thing this capture may not be. It is evidenced by an authored
violation trace instead, where the fault belongs. `VERS-004` used to be its
pair — a capture would have to declare an extension, and this host implements
none, and declaring one only to light up a check is precisely the vacuous
evidence the corpus exists to avoid. The Rust SDK's conformance server
declares one of its own accord, and that is evidence.

**One is a recording gap rather than a session gap**: `TRAN-070` needs a
response stream to close mid-session, and the server's tap records no
lifecycle events at all, so no HTTP capture can carry one however the session
is driven.

**Seven need a protected server**: `AUTH-097`, `AUTH-124` and `AUTH-129`–`AUTH-133`
read a `WWW-Authenticate` challenge, and no committed capture is of a server that
requires authorization. Authored traces carry each (`draft/violations/auth-*`).

**Four need a server that advertises a malformed `x-mcp-header`**: `TOOL-035`,
`TOOL-046`, `TRAN-083` and `TRAN-084` bind a client to reject such a tool and never
call it. This server advertises only what it implements, and every designation it
serves is valid, so no client of it has anything to reject. Authored traces carry
both halves (`tran-082-invalid-tool-excluded-valid-called`,
`tran-083-invalid-tool-called`).

**Twelve need a client feature no captured session uses.** No capture carries a
URL-mode elicitation (`ELIC-064`, `ELIC-065`): the everything server's only one is
`test_url_elicitation`, which it does not offer at `2026-07-28`, and the SDK servers
send none. The one sampling request captured is plain (no `tools`, no
`includeContext`), so tool-enabled sampling and its `toolChoice` and result-pairing
rules (`SAMP-026`, `SAMP-029`–`SAMP-033`, `SAMP-042`) are unreached. No server in any
capture sends `roots/list`, so `ROOT-013`, `ROOT-014` and `ROOT-018` are too. Each
has an authored violation trace under `draft/violations/`.

`BASE-047` used to be on this list. It left without a trace being written,
because it was never a corpus gap: the check behind it counted only
malformed responses as subjects, so every capture full of well-formed results
reported the clause *not observed* — the tool saying it had seen nothing to
judge about a session that had carried dozens of them.

### The official SDKs' conformance servers (`captured/`, `draft/captured/`)

Nine recordings of **the Tier 1 SDKs' own conformance servers** (the TypeScript
SDK's two lines are two servers) — the server its maintainers run the official
suite against — driven over Streamable HTTP by an official SDK client. Neither end is ours: this repository supplies only
the recorder and the judge. They are the evidence behind the book's
[cross-SDK matrix](https://github.com/tomtom215/mcp-conformance/blob/main/book/src/sdk-matrix.md), which `cargo xtask sdk-matrix`
generates from their goldens; the verdicts and counts live there, once, and
are not restated here.

| Field | Value |
|---|---|
| Servers | TypeScript v1: `test/conformance/src/everythingServer.ts` at SDK tag `1.32.1`, its `../../../src/` imports rewritten to the published `@modelcontextprotocol/sdk` 1.32.1 and nothing else. TypeScript v2: the same path at tag `v2.3.1`, unmodified, on the published `@modelcontextprotocol/server` 2.3.1 with `node` 2.1.1 and `express` 2.0.2 (one process serves both revisions: the `initialize` handshake for `2025-11-25`, `server/discover` for `2026-07-28`). Python: `examples/servers/everything-server` at tag `v2.3.0`, on `mcp` 2.3.0. Go: `conformance/everything-server` from the `go-sdk` v1.8.0 module, stateful at `2025-11-25` and stateless at `2026-07-28`, as the Go SDK's own conformance workflow runs it. Rust: `conformance/src/bin/server.rs` at tag `rmcp-v3.7.0`, compiled against `rmcp` 3.7.0 from crates.io with upstream's feature set. Every pin, commit and file hash is in [`interop/sdk-pins.env`](https://github.com/tomtom215/mcp-conformance/blob/main/interop/sdk-pins.env) |
| Clients | `2025-11-25`: the TypeScript SDK (v1) 1.32.1 client ([`ts-client.mjs`](https://github.com/tomtom215/mcp-conformance/blob/main/interop/ts-client.mjs)). `2026-07-28`: the Python SDK 2.3.0 client ([`py-client.py`](https://github.com/tomtom215/mcp-conformance/blob/main/interop/py-client.py)). One client per revision for every server, so two rows of a revision differ by the server alone |
| Recorded by | `mcp-trace-capture http`, in front of each server on fixed loopback ports, by [`interop/matrix.sh`](https://github.com/tomtom215/mcp-conformance/blob/main/interop/matrix.sh), 2026-10-25 (every cell re-recorded that day; the three `2026-07-28` traces that existed before are byte-identical to the 2026-10-13 ones, because a stateless session carries no value that changes between runs) |
| Re-verified | Weekly: the scheduled `sdk-matrix` job re-records every session with the same pins and runs `cargo xtask sdk-matrix --recaptured`, which fails if any re-judged report differs from its golden |

| Trace | Session |
|-------|---------|
| `sdk-typescript-2025-11-25.jsonl` | The TypeScript v1 server at `2025-11-25`: handshake, the standalone `GET` stream, tools, resources, prompts, `logging/setLevel`, an unknown tool, and the `DELETE` teardown. The TypeScript SDK's 1.x line does not serve `2026-07-28`; the Python client's discovery falls back to `initialize` against it, so there is no `2026-07-28` trace for it |
| `sdk-typescript-v2-2025-11-25.jsonl` | The TypeScript v2 server, the same session, answered through its legacy (handshake) path |
| `sdk-python-2025-11-25.jsonl` | The Python server, the same session |
| `sdk-go-2025-11-25.jsonl` | The Go server, the same session. Its first prompt is an MRTR fixture, which it answers to this `2025-11-25` client with a JSON-RPC error |
| `sdk-rust-2025-11-25.jsonl` | The Rust server, the same session |
| `sdk-typescript-v2-2026-07-28.jsonl` | The TypeScript v2 server at `2026-07-28`, the same session as the Python server's: `server/discover` (answered with `supportedVersions` `["2026-07-28"]`), then tools, resources and prompts with the per-request `_meta` envelope |
| `sdk-python-2026-07-28.jsonl` | The Python server at `2026-07-28`: `server/discover`, then tools, resources and prompts with the per-request `_meta` envelope |
| `sdk-go-2026-07-28.jsonl` | The Go server, the same session. Its first prompt answers with an `input_required` result asking for `elicitation/create` from a client that declared no `elicitation` capability — the findings the matrix reports against it |
| `sdk-rust-2026-07-28.jsonl` | The Rust server, the same session |

**The trace bytes are not reproducible, and are not normalized.** Each server
mints a fresh session id on every run, and nothing is rewritten after
recording — a recording edited after the fact is no longer a recording. What
is held fixed instead is everything that changes a *result*: the ports (the
proxy records the `Host` header), the client script, the pins, and the one
race the session had. The TypeScript client opens its standalone `GET` stream
in the background, and while that `GET` and the next `POST` both awaited a
status the validator — correctly — attributed neither, so `TRAN-040` read
*not observed* on some runs and `pass` on others; the client now waits for the
`GET` to be answered before its next request. Five consecutive local runs then
produced identical reports, `seq` for `seq`. The weekly comparison ignores a
finding's `seq` all the same, because a session whose replies land a line
apart is the same session.

Regenerate with `interop/setup.sh` and `interop/matrix.sh`, then
`BLESS=1 cargo xtask sdk-matrix --recaptured interop/out-matrix`,
`cargo xtask bless`, `cargo xtask sdk-matrix` and `cargo xtask draft-coverage`,
and review the golden diff.

### `2026-07-28` authored (`corpus/draft/`)

Fixtures for revision `2026-07-28`, judged by its registry. Hand-authored
rather than captured, so each is a minimal trace written to exercise one
clause, which is what keeps a violation attributable to the check it kills.
Recorded `2026-07-28` sessions are in `draft/captured/` (above), and the
interop job records the official Python SDK's client and server at that
revision and requires them to judge clean
([`interop/README.md`](https://github.com/tomtom215/mcp-conformance/blob/main/interop/README.md)).

Where a trace falsifies more than one requirement, the row says so and why.
There are only three reasons, and none is sloppy authorship: several clauses
state *one* rule across several sections and share a check by design, one
clause's antecedent is itself a violation by the other party — a server can
only fail to reject a bad header if the client sent one — or the shape a trace
exists to carry is a violation in its own right: an HTTP answer with
`"id": null` that the framing ties to its request is BASE-052's whenever that
request's id could be read. Every other row falsifies exactly the requirement
it is named for.

**Every result names its server, and every request its client (2026-10-13).**
When BASE-033, BASE-037 and DISC-003 stopped being excluded, the authored
traces were brought up to the two SHOULDs they had been written without: each
result answering a request under the per-request envelope gained
`_meta.io.modelcontextprotocol/serverInfo` (`corpus-server` `0.1.0`), and the
eight violation traces whose requests lacked `clientInfo` gained it. Nothing
else in any trace changed, and no other row's outcome moved; without the edit
every authored trace would carry a warning unrelated to the clause it exists
for. The three traces named for those clauses are the only ones left without.

**Every form elicitation carries its schema (2026-10-13).** When the
client-feature pages entered the registry (review item 2.5), seven authored traces
were found asking for form mode elicitation without the `requestedSchema` the
page requires, or with no parameters at all — fixtures written for MRTR clauses
that never needed one: `mrtr-004`, `mrtr-015`, `mrtr-016-request-state-not-echoed`,
`mrtr-018`, `mrtr-024`, `tran-066` and `base-034-input-request-for-undeclared-capability`.
Each gained the same `message` and `requestedSchema`; nothing else changed. The
wire schema had been failing all but the last of them for the omission, unrelated
to the clause each exists for; it now passes them, and no existing requirement's
outcome moved.

| Trace | Exercises |
|-------|-----------|
| `stateless-session.jsonl` | Conformant `2026-07-28` stateless session over **stdio**: every request carries its own `_meta` envelope (protocolVersion + clientCapabilities), every result carries `resultType`, request ids are reused only after their response. It closes with two calls in flight, a `notifications/cancelled` for one of them, and the server answering only the other — TRAN-124's pass path, which a recording can only carry by showing a *permitted* message where the forbidden one would be. It also carries a complete conforming MRTR round — `input_required` with an `elicitation/create` request and a `requestState`, then a retry under a new id echoing the state and supplying `inputResponses` — so the MRTR checks pass on real content. The Streamable HTTP clauses report *not observed* here: there is no HTTP framing for them to read, which is why `streamable-http-session.jsonl` exists. |
| `mrtr-parallel-rounds.jsonl` | Two `tools/call` rounds in flight at once — the shape the official Python SDK produces for concurrent calls: both requests, both `input_required` results, then the retries in the *opposite* order, each echoing its own `requestState` and answering its own `inputRequests`. Until retries were paired by the state they echo, the second retry was judged against the most recent round — the other call's — and this conforming exchange failed MRTR-003/015/016/017. |
| `mrtr-retry-round-before-recording.jsonl` | A fresh stdio process receives a `tools/call` retry carrying `inputResponses` and a `requestState` minted by an earlier server process, and completes it; the `input_required` round predates the recording. `requestState` exists so a retry can reach an instance that never saw the round, so MRTR-015/016/018 have no round to judge it against. Until 2026-10-12 MRTR-018 read the missing round as one that issued no state and failed it. |
| `client-features-mrtr.jsonl` | The client-feature traffic of `good/stdio-client-features.jsonl` under MRTR, over stdio (added 2026-10-13, review item 2.5): one `tools/call` whose `input_required` asks four inputs at once — form and URL mode elicitation (no `elicitationId` at this revision), a tool-enabled sampling request under `toolChoice` `required`, and `roots/list` — retried with all four answers; a second round continuing the tool loop under `toolChoice` `none`; then the result. Every request declares `elicitation` (both modes), `sampling.tools` and `roots` in its own `clientCapabilities` |
| `http-authorization.jsonl` | `good/http-authorization.jsonl` at this revision (added 2026-10-13, review item 2.8): `server/discover` refused `401` with `resource_metadata` and `scope` (AUTH-097), resent with the same id and answered; a `tools/list` refused `403` `insufficient_scope` (AUTH-130/131/132), retried with the same id and answered — AUTH-133's pass path, a retry the server did not challenge again — and another refused `401` `invalid_token` (AUTH-124/130). No challenge names `offline_access` (AUTH-129). The same-id retries pass BASE-045: the refused request was never read, so it was never in flight |
| `subscription-server-teardown.jsonl` | A stdio subscription the *server* ends: acknowledgment, one requested change notification, then `notifications/cancelled` with `requestId` naming the `subscriptions/listen` request — the teardown basic/patterns/cancellation makes mandatory ("A server MUST send `notifications/cancelled` referencing a `subscriptions/listen` request ID when it tears down that subscription stream"). It carries no `subscriptionId` tag, since `requestId` already names the subscription; until 2026-10-04 BASE-039 failed it for the missing tag, and SUBS-001 failed the tagged form as an unrequested notification type, so the mandated message could not pass either way. |
| `subscription-id-reused-after-close.jsonl` | A stdio subscription closed by its graceful-closure response, then a second `subscriptions/listen` reusing id 1 with a different filter (`promptsListChanged` only): each opening acknowledged first, each notification one its own filter requested (SUBS-001/002 pass paths). Until 2026-10-12 both checks keyed subscriptions by id alone, so the second filter judged the first stream's `tools/list_changed` and SUBS-001 failed. |
| `caching-ttl-written-as-float.jsonl` | A `tools/list` result carrying `"ttlMs": 60000.0`. The schema types `ttlMs` as JSON Schema `integer`, which admits any number with a zero fractional part, so this is 60000 milliseconds (CACH-001/008 pass paths) — the spelling a serializer that holds every JSON number as a double may produce. Until 2026-10-12 CACH-008 accepted only values that parsed as `i64` and failed it. |
| `traceparent-higher-version.jsonl` | A request `traceparent` of version `01` with a fifth field after the flags, answered with a result whose `traceparent` is downgraded to `00` — the first mutation W3C Trace Context §3.4 allows. §3.2.4 assumes future versions "will be additive" and has a parser check only that the flags are followed by "the end of the string or a dash", so the extra field is valid (BASE-040's pass path). Until 2026-10-12 the check demanded exactly four fields at every version and failed it. |
| `resource-read-existing-empty.jsonl` | A listed resource (`file:///project/empty.log`) read back with an empty `contents` array. RES-022 forbids that shape only "for a non-existent resource", and the page concedes the other reading — "it could mean the resource exists but has no content" — so with nothing in the session saying this resource is missing, nothing is reported. Until 2026-10-04 the shape alone failed the MUST. |
| `base-030-request-meta-missing-required-field.jsonl` | Request `_meta` omits `io.modelcontextprotocol/clientCapabilities` (BASE-030) |
| `base-030-malformed-request-id-reused-correctly.jsonl` | A `tools/list` whose `_meta` omits `clientCapabilities` is correctly rejected with `-32602`; the client then reuses id 1 — legal once answered (BASE-045) — for a well-formed `tools/list`, which is served (BASE-030 for the first request only). The golden pins BASE-031 *passing*: correlated by id text alone, the second request's result was held against the first, malformed one. |
| `base-030-unsupported-version-refused-as-invalid-params.jsonl` | Over stdio, a `tools/list` whose `_meta` omits `clientCapabilities` *and* names `2099-01-01`, which the server's `server/discover` result omits; the server answers `-32602` (BASE-030, with MRTR-002, COMP-006, PAGE-006, TOOL-016, RES-011 and PROM-011 — the same envelope rule restated per feature, sharing its check). Both `-32602` and `-32022` are defined for this request and one answer carries one code, so the golden pins VERS-001 *passing*; until 2026-10-12 it and TRAN-074 failed it. TRAN-074, the Streamable HTTP statement of the rule, is *not observed* on this stdio trace. |
| `input-required-id-reused.jsonl` | A `tools/call` declaring `elicitation` draws an `input_required` asking for `elicitation/create`, is retried and served under id 2, and then id 1 — answered, so free again (BASE-045) — is reused for an unrelated call declaring no capabilities. Until 2026-10-12 capabilities were looked up by request id across the whole trace, so the `input_required` was held against the *last* request with id 1, and this conforming session failed BASE-034, MRTR-012 and MRTR-013. |
| `base-030-malformed-request-refused-without-id.jsonl` | The same malformed `tools/list` over Streamable HTTP, rejected `400` with a `-32602` whose id is null, then retried well-formed under id 1 and served (BASE-030 for the client's envelope; BASE-052 for the null id, since the framing names a request whose id could be read). The golden pins BASE-031, BASE-032 and BASE-045 *passing*: until 2026-10-12 the null-id rejection answered nothing, the malformed request stayed open, and the retry's result was held against it. |
| `base-030-legacy-initialize-negotiated-2026-07-28.jsonl` | A `2025-11-25` client's `initialize`, answered by a server whose result names `protocolVersion` `2026-07-28` (and carries `resultType`); the client then continues under it with a well-formed modern `tools/list` (added 2026-10-24). The session ran under the revision the server negotiated, so `validate` judges it under `2026-07-28` alone — one report, with a note on stderr naming the proposal — where the legacy `initialize` is a client request without its required `_meta` fields (BASE-030; MRTR-002, COMP-006, PAGE-006, TOOL-016, RES-011 and PROM-011 restate that one envelope rule per feature page and share the check) sent by a dual-era client that skipped the `server/discover` probe (DISC-002 and TRAN-128, warnings, one check). Until 2026-10-24 the same trace was judged side by side under both revisions, with a `2025-11-25` `LIFE-003` failure for an `initialized` notification a `2026-07-28` session never owed. |
| `base-031-malformed-meta-answered-with-result.jsonl` | Server answers a `_meta`-incomplete request with a result instead of `-32602` (BASE-031) |
| `base-031-reused-id-malformed-answered.jsonl` | The reverse order: a well-formed `tools/list` id 1 is served, then a `_meta`-incomplete request reusing id 1 is *also* served (BASE-031; BASE-030 for the client's envelope). Exactly one BASE-031 finding, on the second answer — keyed by id alone, the first, legitimate answer was reported too, for a request sent after it. |
| `base-032-invalid-params-not-http-400.jsonl` | `-32602` returned with HTTP 200 rather than 400 (BASE-032) |
| `base-033-request-without-client-info.jsonl` | A `tools/list` whose `_meta` carries the envelope but no `io.modelcontextprotocol/clientInfo` (BASE-033, a warning). Excluded until 2026-10-13. Added 2026-10-13 |
| `base-034-input-request-for-undeclared-capability.jsonl` | Server returns `input_required` asking for `elicitation/create` the request never declared (BASE-034) |
| `base-034-input-request-for-undeclared-mode.jsonl` | The request declares `elicitation: {"form": {}}` and a plain `sampling: {}`; the server asks for a URL-mode `elicitation/create` and a `sampling/createMessage` carrying `tools` (BASE-034; MRTR-012 and MRTR-013 share the check). `client/elicitation`: "Servers MUST NOT send elicitation requests with modes that are not supported by the client"; `client/sampling`: "Servers MUST NOT send tool-enabled sampling requests to Clients that have not declared support for tool use via the `sampling.tools` capability." Only top-level capability names were compared until 2026-10-12, so this passed. |
| `base-035-missing-capability-error-without-capabilities.jsonl` | `-32021` carries no `data.requiredCapabilities` (BASE-035) |
| `base-036-missing-capability-not-http-400.jsonl` | `-32021` returned with HTTP 500 rather than 400 (BASE-036). The `500` also carries no `Content-Type`, which since 2026-10-13 fails TRAN-063 too |
| `base-037-result-without-server-info.jsonl` | A `tools/list` result with no `io.modelcontextprotocol/serverInfo` in its `_meta` (BASE-037, a warning) — what rmcp's servers send on every result but `server/discover` and `subscriptions/listen`, where the Go and Python SDK servers name themselves on every result (`draft/captured/`). Excluded until 2026-10-13. Added 2026-10-13 |
| `base-039-subscription-notification-untagged.jsonl` | Notification on a `subscriptions/listen` stream with no `io.modelcontextprotocol/subscriptionId` (BASE-039) |
| `base-039-subscription-notification-mistagged.jsonl` | A `subscriptions/listen` with the numeric id `1`, acknowledged under tag `1`, then a change notification tagged with the *string* `"1"` (BASE-039). The schema: "The value is the JSON-RPC ID of the `subscriptions/listen` request that opened the stream" — and JSON-RPC ids are compared as values, so `"1"` names no subscription. Only the tag's presence was checked until 2026-10-12. |
| `base-040-malformed-traceparent.jsonl` | `traceparent` that is not W3C Trace Context shaped (BASE-040) |
| `base-040-traceparent-version-ff.jsonl` | `traceparent` with version `ff`, which W3C Trace Context forbids ("Version ff is forbidden"), and otherwise well formed (BASE-040). Passed until 2026-10-12, when the check accepted any two hex digits as the version. |
| `base-045-request-id-reused-in-flight.jsonl` | Request id reused while the first is still outstanding (BASE-045) — legal at `2025-11-25` only after a response, and this trace reuses *before* one |
| `base-048-result-without-result-type.jsonl` | Result omits the `resultType` SEP-2322 requires (BASE-048) |
| `base-052-null-id-error-then-id-reused.jsonl` | A well-formed `tools/call` for an unknown tool answered with a `-32602` whose id is null, then id 1 reused for a call that is served (BASE-052). The status ties the error to its POST, whose request has the readable id `1`, so the escape hatch for an id that "could not be read" does not apply. The golden pins BASE-045 *passing*: until 2026-10-12 the null-id answer closed nothing, and the reuse was reported as in flight. |
| `base-083-legacy-error-code.jsonl` | Error code `-32010` from the legacy sub-range, which new implementations SHOULD NOT use (BASE-083, a warning). Named `base-055-…` until 2026-10-04, when BASE-055 carried this use at MUST NOT alongside the allocation rule it actually states. |
| `base-057-undefined-reserved-error-code.jsonl` | Error code `-32055`: inside the MCP-reserved sub-range but undefined (BASE-057) |
| `base-058-withdrawn-error-code.jsonl` | Error code `-32002`, withdrawn by this revision (BASE-058) |
| `base-060-app-code-in-reserved-range.jsonl` | Application-defined `-32500` placed inside the JSON-RPC reserved range (BASE-060) |
| `streamable-http-session.jsonl` | Conformant `2026-07-28` Streamable HTTP session: `server/discover` under client capabilities advertising a correctly prefixed extension identifier (VERS-004), a paginated `tools/list` whose two pages agree on `cacheScope` (CACH-015/016) and which declares `x-mcp-header` annotations on a string *and* an integer property, a `tools/call` mirroring both into `Mcp-Param-Region`/`Mcp-Param-Limit` — the integer inside the IEEE 754 safe range (TOOL-034) — over an SSE response with `X-Accel-Buffering: no`, and a `resources/read` whose non-ASCII `Mcp-Name` rides the Base64 sentinel. The continuation page is fetched *after* a `transport-close`, which is ordinary here (every POST gets its own response stream) and is the only way a recording can show a server declining to send more for the request that close cancelled — TRAN-070's pass path. |
| `http-concurrent-posts.jsonl` | Two `tools/call` POSTs in flight at once, recorded the way the capture proxy interleaves them — both requests' `http` events, then both bodies, then both statuses and both answers — followed by one serial call. Every header matches its own body. Paired by "the next client message", the second POST's headers landed on the first body and a conforming exchange failed TRAN-058/TRAN-097/TRAN-100; overlapping POSTs are now left unpaired, and the serial call after them pairs and is judged as before. |
| `http-upstream-abort.jsonl` | Two requests in flight; the capture proxy records a `transport-abort` — what it writes when the upstream server fails and it answers the client 502 itself — and the server then answers the other request normally. An abort is a failed transport, not a client closing its stream, and a lifecycle event names no request; until 2026-10-04 TRAN-070 read any close or abort as cancelling every request in flight and failed the server for the second answer. |
| `http-stream-closed-by-client.jsonl` | A `tools/call` whose SSE response stream closes while it is the only request in flight — the cancellation signal at this revision — after which the server sends nothing more for it, and a later `tools/list` is answered normally (TRAN-070's pass path, now that a close is attributed only when exactly one request is in flight). |
| `http-close-after-another-response.jsonl` | A `subscriptions/listen` stream left open while a `tools/call` beside it is answered; a `transport-close` is recorded straight after that answer, and the subscription is later closed gracefully. The final response "SHOULD terminate the stream", so the close is the call's stream ending, not the subscription being cancelled. Until 2026-10-12 TRAN-070 pinned the close on the one request still open and failed the graceful closure. |
| `stdio-discover-preferred-version-unsupported.jsonl` | A stdio `server/discover` probe naming `2027-03-01`, answered with a `DiscoverResult` listing only `2026-07-28`, then a `tools/list` at that version. `basic/versioning`'s compatibility matrix names the outcome: "The stdio probe returns a `DiscoverResult` (or `UnsupportedProtocolVersionError`)". Until 2026-10-12 TRAN-074 and VERS-001 failed the result for not being `-32022`; VERS-001 now passes it, and TRAN-074, an HTTP clause, is *not observed* on stdio. |
| `http-dual-era-modern-request-first.jsonl` | A dual-era client over Streamable HTTP opening with a modern `tools/list`, refused `400` with `-32600`, then falling back to `initialize` (carrying a full `_meta` envelope and left unanswered, as in `disc-002-…`, so nothing else fires). HTTP's own rule lets it "detect which era the server implements by attempting a modern request first"; the probe-first clause is stdio's. Until 2026-10-12 DISC-002 and TRAN-128 warned here. |
| `http-origin-refused-without-id.jsonl` | The `2026-07-28` counterpart: a `tools/list` from `evil.example.com` refused `403` with an id-less error, then served from `localhost`. The same sentence permits the body at this revision, and BASE-052 — BASE-009's restatement, sharing its check — passes it. Added 2026-10-13 |
| `stdio-cancellation-race.jsonl` | A `tools/call` cancelled by notification whose progress and result were already on their way: both land after the cancellation in the recording, then a later `tools/list` is answered. basic/patterns/cancellation names this race ("this notification MAY arrive after the request has already finished"; "Both parties MUST handle these race conditions gracefully"), and nothing shows the server had read the cancellation when it wrote them. Until 2026-10-04 TRAN-124 convicted every message recorded after a cancellation. |
| `stdio-cancelled-id-reused.jsonl` | A `tools/call` id 5 cancelled by notification and answered in the race the cancellation page names; a later `tools/call` id 6 shows the server has read the cancellation; the client then reuses id 5 — legal once answered (BASE-045) — for a new call, which is answered (TRAN-124's pass path). Until 2026-10-12 the stdio check never cleared a cancellation when its id was reused, as the HTTP twin does, and failed the answer to the new request. |
| `stdio-progress.jsonl` | Progress done right over **stdio**: rising values (`0.25`, `0.5`, `1`) for one token, ending before the result; the token reused by a later request once the first was answered, its sequence starting again below where the last one ended; and an integer token running concurrently with it. The pass path of PROG-001/002/003/005/008. |
| `logging-per-request.jsonl` | Per-request logging done right: a declared `logging` capability, a `tools/call` carrying `io.modelcontextprotocol/logLevel` that receives a log while it is the only request in flight, then a second call that sets no level and receives none (LOG-007/008/009 pass paths). |
| `stdio-x-mcp-header-large-integer.jsonl` | The `tool-034` session over **stdio**: an `x-mcp-header`-annotated integer argument of 2^53. Mirroring is a Streamable HTTP mechanism ("Clients using other transports (e.g., stdio) MAY ignore `x-mcp-header` annotations entirely"), so no value is put through a header and TOOL-034 does not bind; until 2026-10-04 it failed the MUST here. |
| `http-x-mcp-header-omitted.jsonl` | The `x-mcp-header` behaviour table's conforming rows over Streamable HTTP: a designated `region` passed as `null` and left out of the arguments, each POSTed without `Mcp-Param-Region` and served, then passed a value and mirrored. The pass path of TRAN-130 through TRAN-135. |
| `tran-058-request-metadata-headers-missing.jsonl` | POST carries neither `Mcp-Method` nor `Mcp-Name` (TRAN-058) |
| `tran-058-mcp-name-mismatch-refused-without-id.jsonl` | A POST whose `Mcp-Name` disagrees with `params.name` (TRAN-058), refused `400` with a null-id `-32020`; the client corrects the header and retries reusing id 1, which is served. Also falsifies BASE-052 — the null id answers a request whose id could be read. The golden pins BASE-045, TRAN-097 and TRAN-100 *passing*: until 2026-10-12 the refusal answered nothing, so the retry was an in-flight reuse and its result was held against the mismatched POST. |
| `tran-060-client-posts-a-response.jsonl` | Client POSTs a JSON-RPC response (TRAN-060). Also falsifies BASE-046: at this revision a server cannot issue the request such a response would answer, so an unsolicited id is the only shape the violation can take. |
| `tran-061-notification-answered-200.jsonl` | A `notifications/cancelled` POST answered `200` instead of `202 Accepted` (TRAN-061) |
| `tran-066-independent-server-request.jsonl` | Server sends `elicitation/create` as its own request on the response stream instead of an MRTR input request (TRAN-066; BASE-084, basic/patterns' "Servers MUST NOT initiate JSON-RPC requests", is the same rule stated for every transport and shares the check) |
| `tran-068-sse-without-accel-buffering.jsonl` | SSE response omits `X-Accel-Buffering: no` (TRAN-068, SHOULD → warn) |
| `tran-068-sse-media-type-mixed-case.jsonl` | The same omission on a response typed `Text/Event-Stream; charset=utf-8` (TRAN-068). Media types are case-insensitive (RFC 9110 §8.3.1); until 2026-10-12 only the lowercase spelling was recognized as SSE, and this response was not judged. |
| `tran-070-message-after-stream-close.jsonl` | Server answers a request whose response stream had already closed, which this revision treats as cancellation (TRAN-070) — the close is attributable because exactly one request is in flight when it is recorded |
| `tran-071-protocol-version-header-missing.jsonl` | POST request without `MCP-Protocol-Version` (TRAN-071) |
| `tran-072-protocol-version-header-mismatched.jsonl` | Header says `2025-11-25`, body `_meta` says `2026-07-28`; the server rejects it correctly, isolating the client's fault (TRAN-072) |
| `tran-072-mismatch-rejected-without-id.jsonl` | The client's header/body version disagreement (TRAN-072), refused with `400` and `-32020` — but with `"id": null`. The golden pins TRAN-073 *passing*: the POST and its answer are the only exchange in flight, so the framing ties the null-id answer to the request, and the right code is the right code. Since 2026-10-12 it also falsifies BASE-052, inseparably from the shape under test: the framing names the request, its id `1` could be read, and only an id that could not be read may be answered with null. |
| `tran-072-unsupported-body-version-header-mismatch.jsonl` | Header `2026-07-28` against a body `_meta` naming `2099-01-01`, a version the server's `server/discover` result omits, refused `400` with `-32020` (TRAN-072, the client's fault). The request falls under both TRAN-073 (`HeaderMismatch`) and TRAN-074/VERS-001 (`UnsupportedProtocolVersionError`), which name no order, so the golden pins all three *passing*; until 2026-10-12 TRAN-074 and VERS-001 failed this answer. |
| `tran-072-unsupported-body-version-refused-as-unsupported.jsonl` | The same request refused with `-32022` listing `2026-07-28` (TRAN-072). The other half of the pair: the golden pins TRAN-073 *passing*, which until 2026-10-12 failed this answer — so neither code could pass before. |
| `tran-073-header-mismatch-not-rejected.jsonl` | The same disagreement, answered with a result (TRAN-073). Necessarily also falsifies TRAN-072 — the client fault *is* this clause's antecedent. |
| `tran-073-mismatch-refused-without-id.jsonl` | The same disagreement refused with `400 {"id":null,"error":{"code":-32600,...}}` — what the official Python SDK's server returns, treating the `2025-11-25` header as a legacy request (TRAN-073; TRAN-072 for the client's header, the antecedent). Paired by id alone the answer belonged to no request and TRAN-073 reported *not observed*. Also falsifies BASE-052 since 2026-10-12, for the same null id: the request it answers had the readable id `1`. |
| `tran-074-unsupported-version-without-supported-list.jsonl` | `-32022` without the `data.supported` list the clause requires (TRAN-074) |
| `tran-074-unsupported-version-accepted.jsonl` | A request naming a version the server's own `server/discover` result omits, answered with a result (TRAN-074) |
| `tran-075-method-not-found-not-404.jsonl` | `-32601` returned with HTTP 200 rather than 404 (TRAN-075) |
| `tran-063-error-without-content-type.jsonl` | The same unknown method refused correctly — `404` and `-32601` — but with no `Content-Type` on the answer (TRAN-063). Added 2026-10-13 with the check's extension to error statuses that carry a JSON-RPC response, which the calibration against mcpsnoop found (`book/src/mcpsnoop.md`) |
| `tran-077-mcp-name-unencoded.jsonl` | Non-ASCII `Mcp-Name` carried unencoded (TRAN-077, TRAN-086, TRAN-087 — one rule stated in three sections, one shared `transport.header-value-encoding` check) |
| `tran-079-x-mcp-header-not-mirrored.jsonl` | `tools/call` supplies a designated argument without its `Mcp-Param-*` header (TRAN-079; TRAN-130, the behaviour table's "Client MUST include the header", shares the check) — and the server serves it, which since 2026-10-12 also fails TRAN-136 ("Server MUST reject the request"): the client's omission is that clause's antecedent |
| `tran-080-x-mcp-header-name-invalid.jsonl` | One `inputSchema` with three annotation faults: a non-token name, a case-insensitive duplicate, and an annotation on a `number` property (TRAN-080) |
| `tran-089-sentinel-markers-miscased.jsonl` | `=?BASE64?…?=` — the sentinel markers must be exactly lowercase (TRAN-089). The server rejects it correctly, so nothing else fires. |
| `tran-092-sentinel-pattern-unencoded.jsonl` | A plain value that matches the sentinel pattern, carried verbatim rather than encoded (TRAN-092) |
| `tran-096-invalid-param-header-accepted.jsonl` | A recognized `Mcp-Param-Region` whose value has leading whitespace, answered with a result (TRAN-096). Also falsifies TRAN-077/086/087 — the unencodable value the server had to reject is itself the client's encoding fault. |
| `tran-077-param-header-value-rejected.jsonl` | The same unencodable `Mcp-Param-Region`, rejected with `-32020` and HTTP 400 as the rule requires (TRAN-077, TRAN-086, TRAN-087 — the client's encoding fault, which is all that is left to fail). TRAN-096's *pass* path: the value a server must reject is itself a client fault, so the conforming half of that clause cannot live in `good/` and is carried here instead. |
| `tran-097-header-body-mismatch-accepted.jsonl` | `Mcp-Param-Region: us-east1` against `arguments.region = "us-west1"`, answered with a result (TRAN-097, TRAN-100 — one rule stated in two sections). Since 2026-10-13 it also fails the client's TRAN-079: a header naming another value does not mirror it |
| `tran-097-integer-header-different-number.jsonl` | `Mcp-Param-Limit: 41` against an integer `arguments.limit = 42`, served (TRAN-097, TRAN-100; TRAN-058 is not involved). The client's own mirroring defect was recorded here until 2026-10-13 as having "no separate clause"; it has one, TRAN-079 ("MUST mirror the designated parameter values"), which the calibration against mcpsnoop surfaced and this trace now also fails. Pins that the numeric comparison TRAN-101 recommends, which since 2026-10-04 lets `42.0` match `42`, still rejects a different number. |
| `tran-097-mismatch-after-concurrent-posts.jsonl` | The same session as `good/http-concurrent-posts.jsonl`, except the final, serial POST carries `Mcp-Name: beta` against `params.name = "alpha"` and is served (TRAN-097, TRAN-100; TRAN-058 for the client's header — the antecedent of a failure to reject). Pins that the ambiguity rule withholds judgement only on the overlapping POSTs: once they drain, pairing — and conviction — resume. |
| `tran-098-header-mismatch-without-400.jsonl` | `HeaderMismatch` returned with HTTP 500 rather than 400 (TRAN-098, TRAN-102 — one rule stated in two sections) |
| `tran-132-null-parameter-header-sent.jsonl` | A designated `region` passed as `null` (the schema allows it) yet mirrored as `Mcp-Param-Region: null`, which the behaviour table's `null` row forbids ("Client MUST omit the header", TRAN-132). The server serves it, which is not a fault: with no value in the body there is nothing for the header to mismatch. |
| `tran-133-omitted-null-header-rejected.jsonl` | The conforming form of the same call — `region: null`, no `Mcp-Param-Region` — refused with `400` and `-32020`, every other header matching its body: the server expected a header the table says it must not (TRAN-133). |
| `tran-134-absent-parameter-header-sent.jsonl` | The table's other valueless row: `region` left out of the arguments, yet `Mcp-Param-Region: us-west1` sent — a header for a value the body does not carry (TRAN-134). |
| `tran-135-omitted-absent-header-rejected.jsonl` | `region` left out of the arguments and its header correctly omitted, refused with `400` and `-32020` (TRAN-135). The `null` row and this one are judged by separate checks, so neither finding is reported against the other's scenario. |
| `tran-136-missing-param-header-accepted.jsonl` | A designated `region: "us-west1"` sent without `Mcp-Param-Region` and served with a result instead of rejected (TRAN-136; TRAN-079 and TRAN-130 for the client's omission, the antecedent). The shape of the session that showed the gap on 2026-10-12: `header_body_match_validated` compares only headers that are present, so a missing one was judged by nothing on the server's side. |
| `tran-079-param-header-value-not-mirrored.jsonl` | `Mcp-Param-Region: us-east1` against `arguments.region = "us-west1"`, refused with `400` and `-32020` as the server must — so the one defect is the client's: the header does not mirror the value (TRAN-079, `transport.x-mcp-header-value-mirrored`). Until 2026-10-13 such a client was judged conformant, the mismatch reported only as a server's failure to reject it; the calibration against mcpsnoop (`book/src/mcpsnoop.md`) found it. Added 2026-10-13 |
| `tran-079-missing-param-header-rejected.jsonl` | The same omission refused with `400` and `-32020` (TRAN-079 and TRAN-130 for the client). The golden pins TRAN-136 *passing* — its pass path can only exist beside a client fault, the way `tran-072-mismatch-rejected-without-id.jsonl` carries TRAN-073's. |
| `tran-074-unsupported-version-without-400.jsonl` | `-32022` returned with HTTP 200 rather than 400 (TRAN-074). The status half of that clause had no trace of its own until `transport.unsupported-version-status` was split out; it had been riding the kills of the sibling rules it was bundled with. |
| `tool-019-tools-undeclared.jsonl` | `tools/call` answered though discovery declared no `tools` capability (TOOL-019) |
| `tool-020-declared-tools-list-unimplemented.jsonl` | `tools` declared, but `tools/list` refused with `-32601` (TOOL-020) |
| `tool-022-tools-list-order-changes.jsonl` | Two `tools/list` results with the same tools in a different order (TOOL-022) |
| `tool-034-mirrored-integer-out-of-range.jsonl` | An `x-mcp-header`-annotated argument of 2^53, outside the IEEE 754 safe range, sent over Streamable HTTP (TOOL-034). Recorded as stdio until 2026-10-04; the stdio copy is now `good/stdio-x-mcp-header-large-integer.jsonl`. |
| `tool-038-embedded-resource-undeclared.jsonl` | A `tools/call` result embedding a resource with no `resources` capability declared (TOOL-038) |
| `res-012-resources-undeclared.jsonl` | `resources/read` answered though discovery declared no `resources` capability (RES-012) |
| `res-013-declared-resources-list-unimplemented.jsonl` | `resources` declared, but `resources/list` refused with `-32601` (RES-013) |
| `res-022-read-empty-contents.jsonl` | A `resources/read` of `file:///project/gone.txt` answered `-32602` "Resource not found", then a second read of the same URI answered with an empty `contents` array (RES-022). The clause binds a *non-existent* resource, and the not-found answer is the session's own evidence that this one is; without it an empty array is the existing-but-empty reading the page concedes (see `good/resource-read-existing-empty.jsonl`). Rewritten 2026-10-04: the original trace carried the empty array alone. |
| `prom-012-prompts-undeclared.jsonl` | `prompts/get` answered though discovery declared no `prompts` capability (PROM-012). The result it wrongly serves is itself well formed — base64 audio with a valid MIME type — which is PROM-017's pass path |
| `prom-013-declared-prompts-list-unimplemented.jsonl` | `prompts` declared, but `prompts/list` refused with `-32601` (PROM-013) |
| `comp-007-completions-undeclared.jsonl` | `completion/complete` answered though the `server/discover` result declared no `completions` capability (COMP-007) |
| `log-007-logging-undeclared.jsonl` | `notifications/message` emitted though discovery declared no `logging` capability (LOG-007) |
| `log-008-log-without-requested-level.jsonl` | A log notification in a session where no request set `io.modelcontextprotocol/logLevel` (LOG-008) |
| `log-008-log-for-request-without-level.jsonl` | The same session, except the second call — no `logLevel`, and the only request in flight — receives a log (LOG-008). An earlier request *did* ask for logs, so the session-wide reading passed this until 2026-10-04; with one request in flight the notification can only be that request's. |
| `log-009-log-on-subscription-stream.jsonl` | A log notification tagged with a subscription id, so travelling on a subscription's stream (LOG-009) |
| `log-009-log-with-no-request-in-flight.jsonl` | A `tools/call` that asked for logs is answered; with only a `subscriptions/listen` open, the server then sends an untagged log (LOG-009). It travels on no request's response stream, since every request it could belong to has been answered. Passed until 2026-10-12: LOG-009 judged only the subscription tag, and LOG-008 only a log some unopted request could own. |
| `log-010-unrecognized-log-level-accepted.jsonl` | A request declaring log level `verbose`, served rather than rejected with `-32602` (LOG-010) |
| `cach-001-cacheable-result-without-hints.jsonl` | A `complete` `tools/list` result with no `ttlMs` caching hint (CACH-001) |
| `cach-001-ttl-without-cache-scope.jsonl` | A `complete` `tools/list` result carrying `ttlMs` but no `cacheScope` (CACH-001). The page defines "caching hints" as both fields ("Cacheable Results in MCP use two fields to provide caching hints"), and the schema's `CacheableResult` requires both; until 2026-10-04 the check accepted `ttlMs` alone. Three single-issue fixtures that had carried `ttlMs` without a scope (`res-012`, `tool-022`, `tool-034`) gained `"cacheScope":"public"` so they stay single-issue. |
| `cach-008-negative-ttl.jsonl` | `ttlMs: -1`, which servers must never provide (CACH-008) |
| `cach-015-page-scope-changes.jsonl` | A paginated `tools/list` whose second page switches from `private` to `public` (CACH-015, and CACH-016 — the same rule and its worked example) |
| `subs-001-unrequested-notification-type.jsonl` | A `prompts/list_changed` on a subscription whose filter asked only for tools-list changes (SUBS-001) |
| `subs-001-cancelled-for-another-request.jsonl` | A `notifications/cancelled` delivered on a subscription stream (tagged with its id) but cancelling an ordinary `tools/call`, not the subscription — not the teardown the cancellation page permits, so still an unrequested notification on the stream (SUBS-001). Pins that the teardown exemption reads `requestId`, not merely the method. Since 2026-10-12 the same message also fails CANC-003/CANC-011: a server may cancel only a `subscriptions/listen` request, and this one cancels a call. |
| `subs-002-notification-before-acknowledgment.jsonl` | A notification ahead of `notifications/subscriptions/acknowledged` on the same subscription id (SUBS-002) |
| `subs-002-reused-id-first-opening-unacknowledged.jsonl` | Subscription 1's first opening sends `tools/list_changed` ahead of its acknowledgment and is closed; a second `subscriptions/listen` reusing id 1 is acknowledged correctly (SUBS-002, on the first opening). Passed until 2026-10-12, when the second request replaced the first in an id-keyed map and the first opening's messages were never examined. |
| `subs-006-graceful-close-result-not-empty.jsonl` | A graceful-closure response carrying `delivered` alongside `resultType` (SUBS-006, and SUBS-005 — one rule stated in the cancellation list and again under Graceful Closure) |
| `mrtr-004-input-required-on-unsupported-method.jsonl` | `input_required` answering a `resources/list`, which is not one of the three requests that may draw one (MRTR-004) |
| `mrtr-006-input-request-unknown-method.jsonl` | `inputRequests` asking for `tools/list`, which is not an ElicitRequest, CreateMessageRequest or ListRootsRequest (MRTR-006) |
| `mrtr-011-input-required-empty.jsonl` | `input_required` carrying neither `inputRequests` nor `requestState`, opening a round that cannot be completed (MRTR-011) |
| `mrtr-015-retry-without-input-responses.jsonl` | Retry echoes the state but supplies no `inputResponses` for the input it was asked for (MRTR-015). The server then *asks again* rather than erroring — MRTR-024's pass path, which needs a shortfall to answer and so cannot occur in a conforming session |
| `mrtr-016-request-state-not-echoed.jsonl` | Retry rewrites `requestState` instead of echoing it (MRTR-016). Also falsifies MRTR-003 and MRTR-017 by design — the same rule stated from the other side, "MUST NOT modify", sharing one check. |
| `mrtr-016-parallel-retry-alters-state.jsonl` | The same two interleaved rounds as `good/mrtr-parallel-rounds.jsonl`, with one retry presenting an edited `requestState` (MRTR-016; MRTR-003/017 by design). An altered state matches no round, so the retry is paired by the request it repeats — tool `lookup` — and judged against that round, not the more recent one for `summarize`. |
| `mrtr-018-unsolicited-request-state.jsonl` | Retry invents a `requestState` for a round that issued none (MRTR-018) |
| `mrtr-019-retry-reuses-id.jsonl` | Retry reuses the original request's JSON-RPC id (MRTR-019) — legal under BASE-045, since the first was already answered, and forbidden here |
| `mrtr-020-request-state-on-another-method.jsonl` | A `prompts/get` carrying the `requestState` a `tools/call` round issued (MRTR-020) |
| `mrtr-024-shortfall-answered-with-error.jsonl` | Retry omits requested input and the server answers `-32602` rather than asking again (MRTR-024). Necessarily also falsifies MRTR-015 — the client's shortfall is this clause's antecedent. |
| `tran-123-cancellation-without-request-id.jsonl` | `notifications/cancelled` carrying only a reason, naming no request to cancel (TRAN-123; CANC-005 is the cancellation page's statement of the same stdio rule and shares the check) |
| `tran-124-message-after-cancel-notification.jsonl` | Server answers a request the client cancelled by notification (TRAN-124) — *after* answering a `tools/list` the client sent later than the cancellation, which on stdio's ordered stream proves the server had read it. stdio's cancellation signal is the notification, not a stream close, so `transport.no-messages-after-cancellation` — which anchors on a `transport-close` — cannot see this and would have passed it vacuously. Rewritten 2026-10-04: the original answered immediately after the cancellation, the race the cancellation page says implementations must tolerate (see `good/stdio-cancellation-race.jsonl`). |
| `canc-003-server-cancels-tool-call.jsonl` | A stdio server sends `notifications/cancelled` for the client's `tools/call` ("server busy"). At this revision a server cancels nothing but a `subscriptions/listen` request (CANC-003, and CANC-011 — the Behavior Requirements restatement — sharing the check). The pass path is `good/subscription-server-teardown.jsonl`. |
| `prog-001-progress-token-not-integer.jsonl` | A request opting into progress with `progressToken: 1.5`, a number but not an integer (PROG-001). |
| `prog-002-progress-token-reused-in-flight.jsonl` | Two `tools/call` requests in flight at once under the same progress token, so a notification could not say which it reports on (PROG-002). Reuse after an answer is legal, and `good/stdio-progress.jsonl` does it. |
| `prog-003-progress-value-decreases.jsonl` | Progress `50` then `10` for one token (PROG-003). |
| `prog-008-progress-after-completion.jsonl` | A progress notification for a stdio request after its result (PROG-008, and PROG-005 — the token no longer names an active request — sharing the check). |
| `disc-001-server-discover-method-not-found.jsonl` | `server/discover` answered with `-32601`, which this revision makes mandatory (DISC-001, and VERS-003 — the same rule stated under versioning). The client then falls back to `initialize`, carrying a full `_meta` envelope so the trace isolates the fallback: a dual-era client that probed *first* is DISC-002/TRAN-128's pass path, and it can only be witnessed against a server that refused the probe |
| `disc-002-dual-era-client-skips-probe.jsonl` | A client that speaks both eras — a modern `_meta` request, then an `initialize` fallback — whose first request is `tools/list` rather than the `server/discover` probe (DISC-002). The `initialize` carries a full `_meta` envelope so the trace isolates the missing probe rather than also failing BASE-030, and the handshake is left unanswered so no legacy-shaped result has to be judged for `resultType`. |
| `disc-003-discover-result-without-server-info.jsonl` | A `server/discover` result with no `serverInfo` in its `_meta` (DISC-003, a warning; BASE-037 warns too — the general rule, of which this is the restatement for one result type). Every captured server includes it here. Excluded until 2026-10-13. Added 2026-10-13 |
| `vers-002-retry-with-unsupported-version.jsonl` | After a `-32022` offering `2026-07-28`, the client retries with `1899-01-01` — a version the list it was just handed does not contain (VERS-002) |
| `vers-004-extension-identifier-without-prefix.jsonl` | Client capabilities advertise the extension `ui`: a valid `_meta` key, but extension identifiers require the prefix a `_meta` key may omit (VERS-004) |
| `base-073-input-schema-invalid.jsonl` | BASE-073 (the same invalid `inputSchema`, at this revision) |
| `base-088-input-schema-draft-07.jsonl` | BASE-088, a warning (the same valid draft-07 `inputSchema` as `base-086-…`, at this revision) |
| `tran-141-mcp-name-header-missing.jsonl` | A `tools/call` POST carrying `Mcp-Method` but not `Mcp-Name`, which the Standard Request Headers table requires for `tools/call` ("These headers are REQUIRED for compliance", TRAN-141; TRAN-058 shares the check) |
| `tool-045-x-mcp-header-in-defs.jsonl` | TOOL-045 (an `x-mcp-header` annotation on a `$defs` entry the property reaches by `$ref`; TRAN-137 and TRAN-082, the transport page's statements of the same rule, share the check) |
| `tran-082-x-mcp-header-under-any-of.jsonl` | TRAN-082 (the annotation inside an `anyOf` branch, a composition keyword the chain must not pass through; TRAN-137 and TOOL-045 share the check) |
| `tran-082-invalid-tool-excluded-valid-called.jsonl` | Over Streamable HTTP, a `tools/list` serving `deploy`, annotated inside an `anyOf` (TRAN-082; TRAN-137 and TOOL-045 share the check), beside a valid `echo`; the client calls `echo` only. That is the behaviour TRAN-083/TRAN-084 ask for — "a single malformed tool definition does not prevent other valid tools from being used" — and the golden pins it *passing*. Added 2026-10-13 |
| `tran-083-invalid-tool-called.jsonl` | The same invalid `deploy`, served alone, and the client calls it (TRAN-083 with TOOL-046, its restatement on the tools page, and TRAN-084 with TOOL-035: a client that rejected the definition and excluded the tool could not have used it). TRAN-082, TRAN-137 and TOOL-045 fail too — the server's half of the same definition. Excluded until 2026-10-13. Added 2026-10-13 |
| `tran-137-x-mcp-header-under-items.jsonl` | TRAN-137 (the annotation on an array's `items` schema, not a property reachable by `properties` keys alone; TRAN-082 and TOOL-045 share the check) |
| `tool-040-structured-content-not-conforming.jsonl` | TOOL-040 (`structuredContent` missing `conditions`, which the tool's `outputSchema` requires) |
| `wire-schema-resource-missing-uri.jsonl` | Wire schema: a `resources/list` result whose resource has no `uri`, which `Resource` requires; the result carries both caching hints so the trace isolates the schema defect |
| `wire-schema-cache-scope-not-in-enum.jsonl` | Wire schema: `cacheScope: "shared"`, which is neither `public` nor `private` |
| `wire-schema-content-block-unknown-type.jsonl` | Wire schema: a `tools/call` result whose content block has `type: "txt"`, matching no `ContentBlock` member |
| `wire-schema-tool-image-data-not-base64.jsonl` | Wire schema: the same image whose `data` is `not base64!`, in a `2026-07-28` `tools/call` result (added 2026-10-24); this revision's schema marks `ImageContent.data` with `format: byte` as `2025-11-25`'s does, and both are compiled with the same assertion |
| `vers-008-initialize-error-without-versions.jsonl` | A modern-only server refuses a legacy `initialize` with a bare `Method not found`, naming none of the versions it does speak (VERS-008). Necessarily also falsifies BASE-030: a legacy handshake carries no `_meta` envelope — that is what makes it legacy — so the antecedent of this clause is a client that cannot satisfy BASE-030. BASE-031 is *not* also reported, and this trace is why the check was narrowed: `basic/versioning`'s compatibility matrix makes the rejection code implementation-defined for exactly this exchange. |
| `elic-057-answered-without-capability.jsonl` | ELIC-057: the call declares `elicitation`, but its retry — the request carrying the accepted form — declares only `sampling`; the page asks for the declaration "on each request" |
| `elic-059-url-mode-undeclared.jsonl` | ELIC-059: URL mode asked of a request declaring form mode only. BASE-034, MRTR-012 and MRTR-013 make the same observation through `meta.no-undeclared-capability-reliance` |
| `elic-060-request-without-message.jsonl` | ELIC-060: an elicitation with no `message`; the wire schema too |
| `elic-062-form-without-schema.jsonl` | ELIC-062: form mode with no `requestedSchema`; the wire schema too |
| `elic-064-url-mode-without-url.jsonl` | ELIC-064: URL mode with no `url`; the wire schema too |
| `elic-065-url-not-a-valid-url.jsonl` | ELIC-065: a `url` whose query carries an unencoded space (`account=Jane Doe`), which RFC 3986 admits only as `%20`. The wire schema passes it: `format` is not asserted (ADR-0020) |
| `elic-095-accepted-content-violates-schema.jsonl` | ELIC-095, a warning: `accept` with empty `content` against a schema requiring `city` — the shape the reference host's captures showed until it was fixed on 2026-10-13 (below) |
| `samp-026-tools-without-sampling-tools.jsonl` | SAMP-026: `tools` asked of a request declaring plain `sampling`. BASE-034, MRTR-012 and MRTR-013 make the same observation |
| `samp-027-answered-without-capability.jsonl` | SAMP-027: a retry declaring only `elicitation` answers the sampling request its call drew |
| `samp-028-include-context-deprecated.jsonl` | SAMP-028, a warning: `includeContext` `thisServer` with `sampling.context` declared — SAMP-029's pass path |
| `samp-029-include-context-undeclared.jsonl` | SAMP-029, a warning: `includeContext` `allServers` without `sampling.context`; SAMP-028 warns too |
| `samp-030-tool-results-mixed.jsonl` | SAMP-030: a user message mixing text with a tool result; SAMP-031 and SAMP-042 as for `samp-008` at `2025-11-25` |
| `samp-031-tool-use-unanswered.jsonl` | SAMP-031: a tool use followed by a plain text message; SAMP-042 restates it |
| `samp-032-required-tool-choice-ignored.jsonl` | SAMP-032: `toolChoice` `required` answered with text |
| `samp-033-none-tool-choice-ignored.jsonl` | SAMP-033: `toolChoice` `none` answered with a tool use |
| `samp-034-message-without-role.jsonl` | SAMP-034: a sampling message with no `role`; the wire schema too |
| `root-013-answered-without-capability.jsonl` | ROOT-013: a retry declaring only `sampling` answers `roots/list` |
| `root-014-root-not-file-uri.jsonl` | ROOT-014: a root listed as `https://example.com/repos/trips` |
| `root-018-asked-without-capability.jsonl` | ROOT-018, a warning: `roots/list` asked of a request that did not declare `roots`. BASE-034, MRTR-012 and MRTR-013 state the MUST behind it and fail |
| `auth-097-challenge-without-scope.jsonl` | AUTH-097, a warning: the `401` challenge carries `resource_metadata` but no `scope` |
| `auth-124-invalid-token-answered-403.jsonl` | AUTH-124: `error="invalid_token"` sent with `403`; AUTH-130 fails too, as for `auth-045` at `2025-11-25` |
| `auth-129-offline-access-challenged.jsonl` | AUTH-129, a warning: the `401` challenge asks for `files:read offline_access`. The clause's other half, Protected Resource Metadata's `scopes_supported`, is not in a recording |
| `auth-130-insufficient-scope-answered-401.jsonl` | AUTH-130: `error="insufficient_scope"` sent with `401`; AUTH-131 warns too |
| `auth-131-insufficient-scope-without-resource-metadata.jsonl` | AUTH-131, a warning: an `insufficient_scope` challenge without `resource_metadata` |
| `auth-132-insufficient-scope-names-no-scope.jsonl` | AUTH-132, a warning: an `insufficient_scope` challenge with no `scope`; AUTH-131 warns too |
| `auth-133-scope-challenged-incrementally.jsonl` | AUTH-133, a warning: a `tools/list` challenged for `tools:list`, retried unchanged, and challenged again for `tools:read`, which the first challenge did not name — the incremental challenging the page says forces "multiple authorization round-trips for a single operation" |

### Extensions at `2026-07-28` (`corpus/draft/`, ADR-0023)

Fixtures for the tasks (`io.modelcontextprotocol/tasks`) and skills
(`io.modelcontextprotocol/skills`) extensions, hand-authored like the rest of
`corpus/draft/` and judged by the `2026-07-28` registry, whose extension clauses
report in each report's `extensions` block and never move its verdict. Every
violation trace here leaves the core verdict `pass` unless its row says
otherwise, which is the separation the corpus exists to show: the extension's
own verdict fails, the report's does not. A `task-…` or `skill-…` trace is named
for the extension clause it falsifies, and the golden tests look for that row in
the extension block.

| Trace | Exercises |
|-------|-----------|
| `tasks-session.jsonl` | Conformant tasks session over **stdio**: `server/discover` declaring the extension; a `tools/list` whose `deploy` tool declares an `outputSchema`; a `tools/call` of it from a declaring client answered with a task, polled `working`, then `input_required` twice with the same `inputRequests` key and request (the documented snapshot), answered once with `tasks/update` (acknowledged empty), then `completed` with its `result`, whose `structuredContent` TOOL-040 judges against the `outputSchema` (the task handle itself is not the tool's result: until 2026-10-13 TOOL-040 held it to the schema and failed this session); a second task `failed` with a JSON-RPC `error` and a `statusMessage`; a third followed on a `subscriptions/listen` stream asking for its `taskIds` (acknowledged, then `notifications/tasks` for it), cancelled with `tasks/cancel`, and the stream closed by a `notifications/cancelled` naming the listen request, not the task; and a non-declaring client refused `-32021` for `tasks/get` and for a task listen. The pass path of every tasks check but TASK-035 |
| `tasks-http.jsonl` | Conformant tasks session over **Streamable HTTP**: a task created by `tools/call`, then `tasks/get` with `Mcp-Name: t-1`, the task's id (TASK-035's pass path) |
| `task-001-task-for-undeclaring-client.jsonl` | TASK-001: a `tools/call` whose request does not declare the extension is answered with a task |
| `task-006-task-without-result-type.jsonl` | TASK-006: a task returned for `tools/call` with `resultType: "complete"`. The core verdict fails too: a complete `tools/call` result without `content` is invalid against the revision's schema |
| `task-007-task-result-type-without-task.jsonl` | TASK-007: `resultType: "task"` on a result with no `taskId` |
| `task-015-get-result-not-a-task.jsonl` | TASK-015: a `tasks/get` result with status `running` and no timestamps or `ttlMs` |
| `task-015-input-required-without-requests.jsonl` | TASK-015: an `input_required` task without `inputRequests` |
| `task-017-get-result-type-not-complete.jsonl` | TASK-017: a `tasks/get` result marked `resultType: "task"`. TASK-028 and TASK-032 fail too: the three methods' results state one rule and share its check |
| `task-022-input-request-key-reused.jsonl` | TASK-022: one `inputRequests` key naming two different elicitations across polls. TASK-023 states the same rule and shares the check |
| `task-024-update-ack-not-empty.jsonl` | TASK-024: a `tasks/update` acknowledged with the task's state rather than an empty result. TASK-030, the same sentence for `tasks/cancel`, shares the check |
| `task-026-input-answered-twice.jsonl` | TASK-026, a warning: a second `tasks/update` answering a key an acknowledged one already answered |
| `task-029-task-cancelled-by-notification.jsonl` | TASK-029: `notifications/cancelled` naming the `tools/call` a task answered |
| `task-033-task-listen-without-declaration.jsonl` | TASK-033: a non-declaring client's listen for task notifications acknowledged instead of refused. TASK-038, the error-handling list's statement of it, shares the check |
| `task-034-progress-on-task-stream.jsonl` | TASK-034: a `notifications/message` on a task's listen stream. The core verdict fails too: the filter did not request it (SUBS-001), and a log message's own clauses (LOG-007 to LOG-009) bind it |
| `task-035-mcp-name-missing.jsonl` | TASK-035: a `tasks/get` POSTed without `Mcp-Name` |
| `task-039-get-without-declaration.jsonl` | TASK-039: a non-declaring client's `tasks/get` answered with the task |
| `task-041-failed-without-status-message.jsonl` | TASK-041, a warning: a `failed` task without `statusMessage` |
| `task-042-failed-without-error.jsonl` | TASK-042: a `failed` task without `error`. TASK-015 (its fifth item) and TASK-045 state the same rule |
| `task-043-tool-error-reported-failed.jsonl` | TASK-043: a tool result with `isError: true` carried by a `failed` task. TASK-008 and TASK-044 state the same rule |
| `task-046-completed-without-result.jsonl` | TASK-046: a `completed` task without `result`. TASK-015 (its third item) and TASK-044 state the same rule |
| `skills-session.jsonl` | Conformant skills session over **stdio**: `server/discover` declaring the extension with `directoryRead: true` and the `resources` capability; a two-page `skills/list` (`pdf-processing` with a manifest of its `SKILL.md` and `templates/invoice.md`, then the nested-prefix `acme/billing/refunds` as `"dynamic"`); `skills/get` of the second, and of an unknown skill refused `-32602`; `resources/list` describing the `SKILL.md` as `text/markdown` with its frontmatter's name and description; `resources/read` of the `SKILL.md`, whose SHA-256 is the manifest's digest and whose frontmatter is the entry's; `resources/directory/read` of the skill root and of `templates/`, each listing carrying its `ttlMs` and `cacheScope`, and of the `SKILL.md` itself refused `-32602`. The pass path of every skills check |
| `skill-003-directory-read-not-implemented.jsonl` | SKILL-003: `directoryRead: true` declared, `resources/directory/read` answered `-32601` |
| `skill-004-directory-read-undeclared.jsonl` | SKILL-004: the client calls `resources/directory/read` on a server that declared no `directoryRead` |
| `skill-005-skills-without-resources.jsonl` | SKILL-005: the extension declared without the `resources` capability |
| `skill-007-entry-uri-not-skill-md.jsonl` | SKILL-007: an entry whose `uri` is a `README.md` |
| `skill-008-skill-md-without-frontmatter.jsonl` | SKILL-008: a `SKILL.md` read back without YAML frontmatter |
| `skill-011-skill-under-other-scheme.jsonl` | SKILL-011, a warning: a skill served under `github://` |
| `skill-012-path-not-ending-in-name.jsonl` | SKILL-012: `skill://pdf-tools/SKILL.md` named `pdf-processing`. SKILL-027 states the same rule and shares the check |
| `skill-013-name-breaks-naming-rules.jsonl` | SKILL-013: a skill named `PDF--Processing` |
| `skill-014-authority-not-reg-name.jsonl` | SKILL-014, a warning: a first path segment with a space |
| `skill-015-prefix-segment-invalid.jsonl` | SKILL-015, a warning: a prefix segment containing `\|` |
| `skill-018-skill-md-not-markdown.jsonl` | SKILL-018, a warning: a `SKILL.md` listed as `text/plain` |
| `skill-019-resource-name-not-skill-name.jsonl` | SKILL-019, a warning: a `SKILL.md` listed under the name `SKILL.md` |
| `skill-020-resource-description-differs.jsonl` | SKILL-020, a warning: a `SKILL.md` listed with a description its frontmatter does not give |
| `skill-026-entry-frontmatter-differs.jsonl` | SKILL-026: the file read back has the manifest's digest but a different `description` from the entry's frontmatter |
| `skill-029-listing-names-collide.jsonl` | SKILL-029, a warning: two skills in one listing named `pdf-processing` |
| `skill-034-entry-without-resources.jsonl` | SKILL-034: an entry without `resources` |
| `skill-035-manifest-without-skill-md.jsonl` | SKILL-035: a manifest that does not list the skill's own `SKILL.md` |
| `skill-036-manifest-file-outside-skill.jsonl` | SKILL-036: a manifest listing `skill://shared/LICENSE` |
| `skill-037-manifest-entry-without-size.jsonl` | SKILL-037: a manifest entry with a digest and no `size` |
| `skill-042-skill-beyond-limits.jsonl` | SKILL-042, a warning: a manifest totalling 16,777,217 bytes, one over the limit |
| `skill-044-list-not-implemented.jsonl` | SKILL-044: `skills/list` answered `-32601` by a declaring server. SKILL-002 states the same rule |
| `skill-045-list-result-type-not-complete.jsonl` | SKILL-045: a `skills/list` result marked `resultType: "partial"` |
| `skill-047-list-without-cache-fields.jsonl` | SKILL-047: a `skills/list` result without `ttlMs` and `cacheScope` |
| `skill-048-get-not-implemented.jsonl` | SKILL-048: `skills/get` answered `-32601` by a declaring server. SKILL-002 states the same rule |
| `skill-049-get-result-type-not-complete.jsonl` | SKILL-049: a `skills/get` result marked `resultType: "partial"` |
| `skill-050-get-names-a-directory.jsonl` | SKILL-050: `skills/get` naming the skill's directory rather than its `SKILL.md` (the server's `-32602` is the conforming answer) |
| `skill-051-unknown-skill-wrong-error.jsonl` | SKILL-051: an unknown skill refused with `-32600` rather than `-32602`. SKILL-071, the error-handling list, shares the check |
| `skill-053-get-without-cache-fields.jsonl` | SKILL-053: a `skills/get` result without `ttlMs` and `cacheScope` |
| `skill-065-directory-result-type-not-complete.jsonl` | SKILL-065: a `resources/directory/read` result marked `resultType: "partial"` |
| `skill-066-directory-read-of-a-file-answered.jsonl` | SKILL-066: `resources/directory/read` of a file the manifest lists, answered with an empty listing. SKILL-071 shares the check |
| `skill-067-skill-directory-refused.jsonl` | SKILL-067: `resources/directory/read` of `templates/`, a directory the manifest puts a file under, refused |
| `skill-100-directory-result-without-cache-fields.jsonl` | SKILL-100: a `resources/directory/read` result without `ttlMs` and `cacheScope` |
