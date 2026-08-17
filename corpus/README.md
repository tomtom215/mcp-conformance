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

Two recordings of **the same client driving the same scenarios**, differing in
one variable: which revision the server was serving. That is what makes the pair
worth committing — a single capture shows what one implementation does, while a
matched pair shows what the *revision* costs, and every difference between the
two reports is attributable to that one change.

| Field | `official-suite-2026-07-28-scenarios.jsonl` | `official-suite-2026-07-28-stateless.jsonl` |
|---|---|---|
| Client | The **official MCP conformance suite**, `0.2.0-alpha.9`, driving its `2026-07-28` scenario set (the pin `cargo xtask draft-readiness` holds) | The same client, the same scenarios, the same run |
| Server | `mcp-everything-server` serving **`2025-11-25`** — held to a revision it does not implement, so genuine non-conformance is the expected content | `mcp-everything-server --protocol-version 2026-07-28`, its stateless mode |
| Recorded by | `mcp-everything-server`'s tap, during `cargo xtask draft-readiness`, 2026-08-17 | same run, second leg |
| Contents | 91 events / 22 POST exchanges | 91 events / 22 POST exchanges |
| Our verdict | 58 pass, **1 fail**, 0 warn, 65 not observed, 148 excluded | **59 pass, 0 fail, 0 warn**, 65 not observed, 148 excluded |
| The official runner's verdict | 23/23 | 23/23 |

Both carry `server/discover`, `tools/list`, `tools/call`, `completion/complete`,
`resources/{list,read}`, `prompts/{list,get}` and progress notifications; every
request carries the revision's per-request `_meta` envelope and its SEP-2243
metadata headers, and neither carries an `initialize` anywhere. Their
`tools/list` results differ by one entry, which is the point of recording both:
`test_url_elicitation` is listed by the legacy server and not by the stateless
one, because URL-mode elicitation is a feature `2026-07-28` removed.

**The one finding.** CACH-001 — the legacy server's `complete` results carry no
`ttlMs`, on `resources/list` and three `resources/read`s. That is our
`2025-11-25` server being held to a revision it does not implement, which is the
correct answer, and the stateless capture is the control that proves the check
reports the server rather than the recording.

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

**Why the runner's 23/23 and our 58-vs-59 are both right.** The suite's
`2026-07-28` scenarios exercise features — list a thing, call a thing, read a
thing — and a `2025-11-25` server answers all of them, because rmcp serves a
per-request-versioned POST whichever revision the handler advertises. The
registry here judges the specification's prose instead, so it sees the one place
the two servers actually differ. Neither instrument is wrong; they are measuring
different things, and this pair is the evidence for that.

The 65 not-observed rows are the honest denominator: of the 124 clauses this
revision's registry can judge, these sessions carried subject matter for 59.
They open no subscription, present no cursor, draw no error, and send no
malformed `_meta`, so those clauses are neither passed nor failed here — they
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
| Recorded by | The host's own `--trace-dir` capture, during `cargo xtask draft-capture`, 2026-08-17 |
| Contents | `server/discover`, a full `subscriptions/listen` lifecycle, a 16-tool sweep with four MRTR rounds (three elicitations and one sampling), and a discovery-driven sweep of everything that is not a tool: `resources/{list,templates/list,read}`, `prompts/{list,get}` for all four prompts, `completion/complete`, and one read of a URI the catalog does not contain |
| Our verdict | **77 pass, 0 fail, 0 warn**, 47 not observed, 148 excluded |

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

**What the 47 not-observed rows still are, and why.** Twenty-three are
Streamable HTTP clauses in the `TRAN-057`…`TRAN-102` band that a stdio
recording structurally cannot carry — the band holds 25 judged clauses, and
the two a stdio session does reach (`TRAN-060`, `TRAN-066`) are judged here.
Nine are server *rejection* rules — `BASE-031`, `BASE-032`, `BASE-035`,
`BASE-036`, `VERS-001`, `VERS-002`, `VERS-008`, `LOG-010`, `PAGE-011` —
reachable only by a client that deliberately sends something malformed, which
this session does not: it is the conforming capture, and a probe session is a
separate recording with a separate expected report. Six need surface this
server does not have (`CACH-015`/`CACH-016` and `PAGE-010` need a catalog
large enough to paginate; `TOOL-033`/`TOOL-034` need an `x-mcp-header`
designation; `PROM-017` needs a prompt carrying audio). `TOOL-022` is the
interesting one: rmcp's client caches `tools/list` under the server's own
`ttlMs`, so a second listing never reaches the wire — a conforming client
cannot exercise the deterministic-order clause within the TTL, which is a
property of the caching feature working rather than a gap to close (the
official suite's runner does not cache, and judges it on both of its
captures). The remaining eight are reachable and not yet driven:
`TRAN-123`/`TRAN-124` cancellation, `TRAN-128` and `DISC-002`'s dual-era
probe, `MRTR-024`, `BASE-040`, `BASE-047`, and `VERS-004`.

#### The HTTP capture of the same session

| Field | `reference-host-2026-07-28-http.jsonl` |
|---|---|
| Client | The same `mcp-reference-host`, driving the identical session over Streamable HTTP |
| Server | `mcp-everything-server --transport http --protocol-version 2026-07-28` |
| Recorded by | **The server's tap**, during `cargo xtask draft-capture`, 2026-08-17 |
| Contents | 151 events — the stdio session's 81 messages plus 70 `http` events carrying status and headers |
| Our verdict | **90 pass, 0 fail, 0 warn**, 34 not observed, 148 excluded |

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

At 89 of the 124 judgeable clauses it is the best-covered capture here. Its 35
not-observed rows are the server-rejection rules a conforming client never
triggers, the pagination and `x-mcp-header` clauses this server's surface does
not reach, and `TOOL-022` (rmcp's client caches `tools/list` under the
server's own `ttlMs`, so a second listing never reaches the wire).

#### The probe session

| Field | `probe-2026-07-28-http.jsonl` |
|---|---|
| Client | `mcp-reference-host --probe`: nine hand-built HTTP requests, each wrong on purpose |
| Server | `mcp-everything-server --transport http --protocol-version 2026-07-28` |
| Recorded by | The server's tap, during `cargo xtask draft-capture`, 2026-08-17 |
| Contents | A `_meta` envelope missing a required field; an unimplemented protocol version and the retry after it; a header/body version mismatch; an unknown method; a log level outside RFC 5424's eight; a fabricated cursor; a tool needing a capability the request never declared; and the removed `initialize` handshake |
| Our verdict | Judged against [`conformance/probe-baseline.json`](../conformance/probe-baseline.json), not for cleanliness |

**A conforming client cannot exercise a rejection rule.** Fifteen clauses of
this revision say what a server owes a request it must *not* serve, and every
one of them reported *not observed* on every recording here, because nothing
had ever sent such a request. This file is that request, nine times over.

The probes are built outside rmcp, and that is the point rather than a
shortcut: rmcp's client is what makes the other captures trustworthy — it
builds the `_meta` envelope, mirrors the SEP-2243 headers, and will not emit an
ill-formed request — so it is structurally incapable of being the probe. The
bytes here are the fixture.

**Its verdict is a ledger, not a pass.** The probe breaks client-side clauses
by construction: `BASE-030` because its first request omits a required `_meta`
field, `TRAN-071`/`TRAN-072` because two probes are about exactly those
headers, `PAGE-010` because a fabricated cursor is a fabricated cursor.
Demanding a clean report would mean demanding a probe that probes nothing. So
every finding is listed in `conformance/probe-baseline.json` with a reason, and
the gate holds the set in both directions: a finding not in the ledger is a new
defect or a regression, and a listed finding that stopped occurring is either a
fix that should retire its entry or a check that quietly stopped firing.

**It has already worked in both directions.** The first probe run drew two
server-side findings — the server served a request naming log level
`"chatty"`, and honoured a cursor it never issued — which went into the ledger
as open defects. When they were fixed, the gate refused the change until their
entries were retired, which is the half of a ratchet that is easy to leave
out. All ten rejection clauses the probe exercises now pass.

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
same gate parses every "N of the M judgeable clauses" claim elsewhere in the
shipped Markdown and fails when one disagrees, which is how the counts in this
section stopped being wrong.

<!-- draft-coverage:begin (generated by `cargo xtask draft-coverage`; do not edit by hand) -->
| Capture | Judged | pass | fail | warn | Not observed |
|---------|-------:|-----:|-----:|-----:|-------------:|
| `official-suite-2026-07-28-scenarios` | 59 | 58 | 1 | 0 | 65 |
| `official-suite-2026-07-28-stateless` | 59 | 59 | 0 | 0 | 65 |
| `probe-2026-07-28-http` | 66 | 54 | 10 | 2 | 58 |
| `reference-host-2026-07-28-http` | 89 | 89 | 0 | 0 | 35 |
| `reference-host-2026-07-28-stdio` | 77 | 77 | 0 | 0 | 47 |
| **Union** | **109** | | | | **15** |

Across all 5 captures, **109 of the 124 judgeable clauses** are evidenced by at least one recording. Each capture's own judged count is what *that* recording carried subject matter for; everything else it reports *not observed* rather than counting it as a pass.

The 15 clauses no capture reaches: `BASE-040`, `BASE-047`, `CACH-015`, `CACH-016`, `MRTR-024`, `PROM-017`, `TOOL-033`, `TOOL-034`, `TRAN-070`, `TRAN-079`, `TRAN-080`, `TRAN-096`, `TRAN-123`, `TRAN-124`, `VERS-004`.
<!-- draft-coverage:end -->

The probe session closed the largest group — the rejection rules — and what
is left divides in two. Eight need server surface this reference does not
have: pagination for `CACH-015`/`CACH-016`, an `x-mcp-header` designation for
`TOOL-033`/`TOOL-034` and `TRAN-079`/`TRAN-080`/`TRAN-096`, and a prompt
carrying audio for `PROM-017`. Seven are conforming client behaviour simply
not driven yet: `BASE-040`'s `traceparent`, `BASE-047`, `VERS-004`'s extension
identifiers, cancellation for `TRAN-070`/`TRAN-123`/`TRAN-124`, and
`MRTR-024`'s shortfall retry. Neither group is a defect; both are work.

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
| `stateless-session.jsonl` | Conformant `2026-07-28` stateless session over **stdio**: every request carries its own `_meta` envelope (protocolVersion + clientCapabilities), every result carries `resultType`, request ids are reused only after their response. It now also carries a complete conforming MRTR round — `input_required` with an `elicitation/create` request and a `requestState`, then a retry under a new id echoing the state and supplying `inputResponses` — so the MRTR checks pass on real content. The Streamable HTTP clauses report *not observed* here: there is no HTTP framing for them to read, which is why `streamable-http-session.jsonl` exists. |
| `base-030-request-meta-missing-required-field.jsonl` | Request `_meta` omits `io.modelcontextprotocol/clientCapabilities` (BASE-030) |
| `base-031-malformed-meta-answered-with-result.jsonl` | Server answers a `_meta`-incomplete request with a result instead of `-32602` (BASE-031) |
| `base-032-invalid-params-not-http-400.jsonl` | `-32602` returned with HTTP 200 rather than 400 (BASE-032) |
| `base-034-input-request-for-undeclared-capability.jsonl` | Server returns `input_required` asking for `elicitation/create` the request never declared (BASE-034) |
| `base-035-missing-capability-error-without-capabilities.jsonl` | `-32021` carries no `data.requiredCapabilities` (BASE-035) |
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
