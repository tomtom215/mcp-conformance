<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Feasibility and value of an active prober, quantified against the registry — 2026-10-24

**Provenance.** A dated report, exempt from the living-document gates and kept as written.
It was produced by a model-run audit agent in the session that also wrote
`docs/handoff/2026-10-24-session-state.md`; the VERIFIED / PARTIAL / INFERRED labels are the
agent's own. The handoff says which of its claims were independently reproduced afterwards.
`$S` in commands stands for that session's scratch directory, which no longer exists.

---

# D — Feasibility and value of an active prober (`mcp-conformance probe`)

Scope: read-only analysis of `/home/user/mcp-conformance` (no cargo, no git changes) and of the published
`@modelcontextprotocol/conformance@0.2.0-alpha.12` tarball, fetched into a fresh directory and read as data.
Every count below is produced by a script under `$S/work/probe/` (Appendix A); `$S` =
`$S`.

## 0. The headline facts (read these first)

1. **A prober already exists, HTTP-only, in the reference host.** `mcp-reference-host --probe`
   (`crates/mcp-reference-host/src/cli.rs:73-78`, `src/probe.rs`, `src/probe/designated.rs`) sends nine
   hand-built `2026-07-28` requests plus a discovered `tools/call` with a designated header omitted, with
   reqwest, asserting nothing itself (`probe.rs:28-31`); its recording
   (`corpus/draft/captured/probe-2026-07-28-http.jsonl`) is judged against a hand-written ledger
   (`conformance/probe-baseline.json`, 12 expected client-side findings) by `cargo xtask draft-capture`
   (`xtask/src/draft_capture.rs:89`, `xtask/src/draft_capture/record.rs:128`). It is the only capture that
   evidences 14 rejection clauses (`corpus/README.md:503-506`). It shipped without an ADR. The question is
   therefore not "build a prober?" but "generalise this one into a product surface, and what would it buy?"
2. **The buy is small and precise.** Of the 311 excluded server-or-both clauses (136 at `2025-11-25`,
   175 at `2026-07-28`), a prober makes **42** judgeable (class a), **26** of them without any trace-format
   change, and only **18** of those (5.8 %) are not already stimulated by a *scored* official-suite scenario
   (8 MUST-family, 10 SHOULD-family). 259 (83 %) stay excluded whatever client you build.
3. **Half the value needs no prober at all.** For TRAN-002/007/051/052 (Origin→403), TRAN-015 (session
   404), RES-019 (resource not found), MRTR-021/023 (InputResponses validation), the stimulus is already
   sent by the suite and already recorded by `mcp-conformance test`; what is missing is the *check*.
   Today those recordings report the clauses `excluded`, e.g. the 403 drawn by `Origin:
   http://evil.example.com` in `corpus/draft/good/http-origin-refused-without-id.jsonl` is judged `pass`
   with TRAN-052 excluded (`corpus/golden/draft/http-origin-refused-without-id.json`: 46 pass, 0 fail,
   320 excluded); its `2025-11-25` twin `corpus/good/http-origin-refused-without-id.jsonl` likewise
   leaves TRAN-007 excluded.

## 1. Trace capability today

What an `http` event records (`crates/mcp-conformance-core/src/trace.rs:112-149`): `method` on client
events only (uppercased; absent on responses and on captures before 2026-08-20, `trace.rs:114-131`),
`status` on server events, and `headers` from a closed allowlist (`trace.rs:214-237`):
`host, origin, accept, content-type, mcp-session-id, mcp-protocol-version, last-event-id, mcp-method,
mcp-name, x-accel-buffering, www-authenticate` plus the prefix `mcp-param-*` (`trace.rs:251`). Keys are
lowercased on read; values verbatim. Never recorded, by construction: `authorization`, `cookie`
(`headers.rs:21-33`), the request target — path and query (ADR-0022 facts 1-2, `0022:29-35`; recording
the path was considered and rejected, `0022:99-103`), `content-length`, `location`, HTTP version, TLS,
connection or stream identity, SSE `id`/`retry`/`event` fields (`crates/mcp-trace-capture/src/framing.rs:147-157`).

Bodies: a `message` event holds parsed JSON only (whitespace, key order and number spelling lost,
`02-architecture.md:106-110`). Over HTTP a body that is not JSON, or JSON the trace cannot hold faithfully
(repeated member, >64-bit integer), is **forwarded and not recorded**, only counted
(`relay.rs:271-282`, `Unrecorded::not_json/altered`, `http.rs:194-199`); an empty body yields no event. Over
stdio a non-JSON line **is** recorded, as a JSON string payload, so the validator can judge it
(`stdio.rs:34-55`, `150-194`); so is an unterminated last line. Raw bytes of malformed JSON are therefore
available on stdio and absent on HTTP.

Connection-level events (`trace.rs:88-98`): `transport-open`, `transport-close`, `transport-abort`, one per
trace, not per exchange. The proxy writes `transport-abort` when the upstream cannot be reached (and answers
502 itself, `relay.rs:386-414`), when the upstream cuts a response after its status (`relay.rs:200-213,
366-380`), and when the client fails mid-body (`relay.rs:122-134`). **A server that never answers has no
representation**: the proxy has no response timeout ("there is no response timeout", capture README
§"Upstream failures as they happened"; only a 10 s connect timeout, `http.rs:130-135`). The validator's
framing treats a `transport-abort` as ending one exchange without a status (`context/framing.rs:284-292`),
and an ambiguous pairing as no pairing (`framing.rs:17-21`). `ts` is informational and never read
(`trace.rs:170-172`; `02-architecture.md:135-148` makes timing conduct unjudgeable "for good").

`Host` through the proxy is the upstream's name unless `--preserve-host` (`relay.rs:58-63`,
`http.rs:112-116`); `mcp-conformance test` sets it automatically for loopback subjects
(`crates/mcp-conformance-cli/src/run.rs:175-180`). `Origin` is always forwarded unchanged. The everything
server's tap sits *inside* its policy layer, so that recorder never sees a 403
(`crates/mcp-everything-server/src/http.rs:54-71`); `mcp-trace-capture` records every status as sent
(`relay.rs:159-173`, `239-248`).

### Fields a prober (or the recorder) would need

| New member | Enables | Who emits | Breaking? |
|---|---|---|---|
| `exchange: n` on client `http`, its `message`, the response `http` and every message it frames | Removes "ambiguous pairing is no pairing"; gives per-stream identity for TRAN-034/036/042/043/047/067 | proxy and prober (both know the exchange) | No: additive |
| `body: "not-json" \| "empty" \| "oversized"` (+ optional bounded `raw` string ≤ 256 B) on an `http` request/response event | TRAN-028/062 and MRTR-022 probes over HTTP (today the non-JSON body is invisible) | prober; proxy could too | No |
| `sse: {id, retry}` on message events, and `stream_end: true` on a per-exchange marker | TRAN-031/044/047, TRAN-036/067 | proxy and prober | No |
| `target_kind: "mcp" \| "oauth-protected-resource" \| "other"` on client `http` (never the path) | AUTH-006/010/011/012, AUTH-091/141/142/144 (PRM) | prober only (it knows what it fetched) | No, but revisits ADR-0022 decision 3 |
| `invalid_utf8: true` on a stdio string payload | TRAN-003 passively | wrapper | No |

Breaking-change assessment (STABILITY.md:44): the trace contract is "a trace that validates against a
release's schema is read by that release's validator"; the schema states "Unknown members are ignored, so
producers may add their own" (`schema/trace-event.schema.json:5`) and the reader tolerates them
(`trace.rs:447-466`). **Optional members are non-breaking. A new `kind` or a new `lifecycle.event` value
is not**: unknown kinds are rejected (`trace.rs:388-397`) and `event` is a closed enum
(`schema:69`), so an old validator would refuse a new trace. Design rule: add members, never enum values.
No clause can be judged on a prober-side timeout marker (architecture boundary above); a non-answer can
only appear in the prober's own run record.

## 2. Candidate table

Population: every core-registry entry with `actor ∈ {server, both}` and an `exclusion` at either revision
(`enumerate.py`; 2025-11-25: 97 server + 39 both = 136; 2026-07-28: 117 + 58 = 175; extension registries
excluded, never scored per ADR-0023). Classification is data in `classify.py` (every id, with a note);
totals from its output (`$S/work/probe/out/classification-totals.txt`).

| Revision | a | b | c | d | total | a by level | a needing a new field | a stimulated by a scored suite scenario |
|---|--:|--:|--:|--:|--:|---|--:|--:|
| 2025-11-25 | 21 | 5 | 109 | 1 | 136 | MUST 10, MUST NOT 3, SHOULD 8 | 11 (4 need only the field) | 3 |
| 2026-07-28 | 21 | 4 | 150 | 0 | 175 | MUST 11, MUST NOT 2, SHOULD 8 | 5 (1 field-only) | 5 (+3 by pending scenarios) |
| both | 42 | 9 | 259 | 1 | 311 | | 16 | 8 |

Per area (a/b/c/d): 2025 — authorization 4/0/10/0, authorization-security 0/0/19/0,
authorization-tokens 0/0/9/0, base 1/0/9/0, completion 1/0/2/0, elicitation 0/0/5/0,
elicitation-security 0/0/18/0, lifecycle 1/0/6/0, logging 1/0/3/0, pagination 0/0/1/0, prompts 1/1/1/1,
resources 1/2/4/0, roots 0/0/2/0, sampling 0/0/3/0, tools 0/2/1/0, transport 3/0/9/0,
transport-streamable-http 8/0/7/0. 2026 — authorization 1/0/11/0, authorization-discovery 3/0/3/0,
authorization-security 0/0/20/0, authorization-tokens 0/0/11/0, caching 0/0/2/0, cancellation 1/0/10/0,
completion 1/0/2/0, elicitation 0/0/4/0, elicitation-security 0/0/18/0, logging 0/0/4/0, messages 1/0/3/0,
meta 0/0/3/0, mrtr-client 3/0/1/0, mrtr-server 0/0/6/0, pagination 0/0/2/0, progress 0/0/3/0,
prompts 1/1/2/0, resources 1/1/4/0, roots 0/0/4/0, sampling 0/0/5/0, schema 0/0/8/0,
statelessness 1/0/3/0, tools 0/2/3/0, transport-http-compat 0/0/2/0, transport-http-endpoint 2/0/3/0,
transport-http-headers 2/0/0/0, transport-http-messages 2/0/2/0, transport-http-validation 2/0/5/0,
transport-http-version 0/0/1/0, transport-stdio 0/0/3/0, versioning 0/0/2/0.

Why 259 are (c): 95 bind the authorization server, the credential or token validation (never recorded,
ADR-0022 fact 1); 36 elicitation-security clauses bind identity binding and browser-side flows; the rest
are semantic judgements ("sensitive", "relevant", "appropriate"), internal dispositions ("MUST NOT rely",
"MUST treat as attacker-controlled"), time (LIFE-015-family, CANC-006-009, TRAN-126), documentation,
intermediaries (TRAN-095/104-106), or two correlated connections (BASE-065, PROM-014/RES-014/TOOL-021).

Class (b), 9: PROM-002/RES-002/TOOL-002 and PROM-015/RES-015/TOOL-024 (listChanged needs the server's
list to change, reachable only through a side-effecting tool the server happens to expose), TOOL-012/043
("validate all tool inputs": calling tools with schema-invalid arguments has side effects and a coercing
tool is a false positive), RES-005 (traversal/malformed-URI reads are scanner territory, and a result
proves nothing without the catalogue).

Class (d), 1: **PROM-010** (2025-11-25, SHOULD, "Both parties SHOULD respect capability negotiation") is
excluded as "mechanized at the protocol level by LIFE-009's negotiated-capabilities-only check" — i.e.
fully covered by `lifecycle.negotiated-capabilities-only`; at `2026-07-28` the same sentence sits inside
the *checked* PROM-020 (`checks=['prompts.arguments-validated']`). The exclusion should become a shared
check. Fourteen more exclusions say "judged under another id" (flag X: AUTH-044, BASE-011, BASE-082,
BASE-085, LIFE-008, PAGE-004, TOOL-013; AUTH-123, BASE-061, CANC-004, CANC-012, MRTR-025, TOOL-028, and
PROM-010); they are restatements excluded rather than shared, a policy inconsistency with
`checks/mod.rs:14-18` ("Requirements share a check only where they state one rule across several
sections") but not judgment gaps. **Stale rationales (flag R, text bugs whatever the class):**
TRAN-028 ("no request/response correlation" — false since `context/framing.rs`), TRAN-062 ("a declined
request produces no MCP message to judge" — it produces an attributed exchange), TRAN-052 ("the 403 is
outside what the tap records" — true of the everything server's tap, false of `mcp-trace-capture`),
TRAN-006 ("not request lines" — `method` is recorded now), TRAN-001/TRAN-115 ("violations surface as
capture-time parse failures" — the wrapper now records the fragments, which fail TRAN-004/TRAN-117).

### 2a. The class-(a) stimuli

Notation: C = conforming answer, V = violating answers, F = trace fields read, FP = false-positive risk.
All HTTP probes carry `Content-Type: application/json`, `Accept: application/json, text/event-stream`, and
at `2026-07-28` the `_meta` envelope and `Mcp-Method` mirror (`probe.rs:41-42, 186-206`).

**G1 — Origin/Host refusal: TRAN-002, TRAN-007 (2025); TRAN-051, TRAN-052 (2026).** Quote (TRAN-007):
"If the `Origin` header is present and invalid, servers MUST respond with HTTP 403 Forbidden."
Stimulus: the suite's own (`dist/index.js` at offset ≈516761, `run()`): one POST (`initialize`, or
`server/discover` at 2026) with `Host: evil.example.com`, `Origin: http://evil.example.com`, then the same
with the server's real host. C: 403 (the registry's own corpus shape: `corpus/good/http-origin-refused-
without-id.jsonl` seq 0-3). V: any 2xx, or a JSON-RPC result. F: client `http.headers.origin/host`,
response `http.status` via `Framing::exchanges()`. FP: a server legitimately configured to accept any
origin (public, authenticated, behind a trusted proxy) — the suite scopes the scenario to loopback URLs
without TLS/auth and so must the check (subject = loopback, judged only when the trace shows a loopback
`Host` elsewhere); through the proxy the probe needs `--preserve-host`; `Origin` absent is conforming
(`http.rs:148-152` in the everything server). Note the clause binds `Origin`; **no registry quote names
`Host`** (grep, Appendix A) — a Host-only refusal check has no clause.

**G2 — Input the server cannot accept: TRAN-028 (2025), TRAN-062 (2026), both MUST.** Quote: "If the
server cannot accept the input, it MUST return an HTTP error status code (e.g., 400 Bad Request)."
Stimulus (three bytes-level variants): (i) `{"hello":"world"}` (JSON, not JSON-RPC: recorded as an
`Invalid` message, TRAN-026 fires on the client as expected); (ii) `{` (not JSON: **invisible over HTTP
today** — needs `body: "not-json"`); (iii) `{"jsonrpc":"1.0","id":1,"method":"ping"}`. C: 4xx (with or
without a JSON-RPC error body). V: 2xx, 202. F: paired exchange status; for (ii) the new body marker.
FP: a server that answers 200 with `-32600 Invalid Request` (JSON-RPC 2.0's own convention) is a literal
violation of the MCP sentence — keep that in the finding text; servers that close the connection without
a status record as `transport-abort` and must read *not observed*, not fail.

**G3 — Protocol errors should get a JSON-RPC error: MRTR-022 (2026, SHOULD; stdio and HTTP).** Quote:
"Protocol errors (malformed JSON, invalid schema, internal server errors) SHOULD return a JSON-RPC error
response with an appropriate error code and message." Stimulus: over stdio the line `{` then `\n`
(recorded verbatim as a string payload; TRAN-118's client check fires, expected), and a schema-invalid
request (`{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"_meta":{…},"name":7}}`). C: an error
object (`-32700`/`-32602`; `id: null` is permitted by BASE-052's escape hatch and the correlation check
excuses it only when the request's id was unreadable, `checks/base/correlation.rs:96-104, 123-137`).
V: a result, nothing at all (stdio), an HTTP 4xx with no JSON-RPC body. F: next server message after the
stimulus (stdio has no framing: the check must pair by adjacency and null id — the one place a prober
needs a new pairing rule). FP: TRAN-062 permits an HTTP error status with no body, so over HTTP only the
SHOULD warns; over stdio a server that ignores unparseable lines silently is a warning, not a failure.
This is the only stdio-side robustness probe in the whole table.

**G4 — Error-shape family (list first, then ask for what is not there):** LOG-003 (2025, SHOULD:
`logging/setLevel` `"level":"chatty"` → `-32602`; the 2026 twin LOG-010 is already checked),
PROM-007/PROM-019 (`prompts/get` of a name absent from `prompts/list`, and of a listed prompt with a
required argument omitted → `-32602`), COMP-003/COMP-008 (`completion/complete` whose `ref` names an
unlisted prompt → `-32602`), RES-008 (2025, SHOULD: `resources/read` of `probe://nonexistent/<fixed>` →
`-32002`), RES-019 (2026, **MUST**: same → `-32602`; RES-022 already convicts the empty-`contents` shape).
C: the named code. V: a result; another code. F: the paired response's `error.code` and the listing
result earlier in the trace (the check must read `prompts/list` / `resources/list` /
`resources/templates/list` to establish "not there"). FP: a server whose listing is filtered per client
or incomplete (PROM-013/RES-013 bind the list to "the set currently available to the requesting
client", so at 2026 this is the server's own statement); a URI matching a template (choose a scheme no
template uses); lenient servers answering unknown prompts with an empty completion (SHOULD-level only).

**G5 — Session termination: TRAN-015 (2025, MUST).** Quote: "The server MAY terminate the session at any
time, after which it MUST respond to requests containing that session ID with HTTP 404 Not Found."
Stimulus (the suite's `server-session-lifecycle`, offset ≈363601): `initialize` → id S; `DELETE` with
`Mcp-Session-Id: S`; if 2xx, `POST tools/list` with S. C: 404. V: 2xx or anything else. F: client
`http.method` (`DELETE` is recorded, `framing.rs:258`), `mcp-session-id` on both sides, statuses. FP: a
405 to the DELETE means not terminated — not observed; the client-initiated termination is the only
antecedent a prober can create. TRAN-014 (400 without a session id) stays (c): "servers that require a
session ID" is policy — a server that assigns one and tolerates its absence is conforming.

**G6 — SEP-2243 decoding and numeric comparison: TRAN-090, TRAN-091, TRAN-103 (MUST), TRAN-101 (SHOULD),
2026.** Quote (TRAN-091): "servers MUST decode an encoded `Mcp-Name` or `Mcp-Param-{Name}` value before
comparing it to the corresponding request body value". Stimulus: `tools/call` with
`Mcp-Name: =?base64?dGVzdF9lY2hv?=` for `"name":"test_echo"` (and `Mcp-Param-Limit: 42.0` for
`"limit":42`). C: served (a result). V: `-32020` / 400 — the server compared without decoding. F: the
paired POST's headers and body via `posts_by_message`, the answer's code. Today
`transport.header-body-match-validated` skips every rejected exchange (`validation.rs:207`), so the
inverse is unjudged. FP: none structural (the sentinel is normative); the pending suite scenario
`http-custom-header-server-validation` covers it but is `not_scored: pending`.

**G7 — MRTR validation (2026): MRTR-021, MRTR-023 (SHOULD).** Stimulus: after an `InputRequiredResult`,
retry with `inputResponses` of the wrong shape (MRTR-021: C = `-32602` or a new `InputRequiredResult`, V =
a normal result) and, separately, a correct retry plus one unknown key (MRTR-023, differential: V = a
rejection only when the key is present). F: the two paired exchanges and the earlier `input_required`
result. FP: a server that re-asks instead of erroring is conforming (MRTR-024); a tool with side effects
runs twice — the prober must use the server's own interactive fixture or an operator-named tool. Both are
scored suite scenarios (`input-required-result-validate-input`, `-ignore-extra-params`).

**G8 — No response to a notification: BASE-011 (2025), BASE-061 (2026), MUST NOT.** Stimulus: POST a
well-formed notification the server has no handler for (`notifications/probe`), and `notifications/
cancelled` for an unknown id. C: 202 and no message. V: a JSON-RPC response framed on that status (the
framing ties a null-id answer to the notification POST, `framing.rs:371-384`). FP: a 400 with a
JSON-RPC error body for a malformed notification is common SDK behaviour and literally a response; keep
the probe to well-formed notifications. CANC-017 (SHOULD, "invalid cancellation notifications SHOULD be
ignored") shares the stimulus: V = 4xx or a reply.

**G9 — Needs a new field.** AUTH-006/010/011/012 and AUTH-091/141/142/144 (PRM: after a 401, fetch
`resource_metadata` or `/.well-known/oauth-protected-resource`; C = a JSON document with non-empty
`authorization_servers`; needs `target_kind`; only for servers that challenge). TRAN-031/044/047 (SSE ids:
prime event, uniqueness across streams, no cross-stream replay on `Last-Event-ID`; need `sse.id` and
`exchange`). TRAN-034/036/042/043/067 (which stream carried what, and when it closed; need `exchange` and
a stream-end marker; **no stimulus required** — any recording benefits once the recorder emits them).

**G10 — Low value, listed for completeness.** LIFE-012 (SHOULD, "latest version supported": two
`initialize` sessions — an unsupported version, then a known revision newer than the one answered; needs
cross-session judgment). BASE-063 (MUST NOT rely on prior requests: vary `clientCapabilities` between
requests on one connection; the consequence is BASE-035's `-32021`, already checked).

### 2b. Ranked (a) list — value to server authors × ease

1. **TRAN-007/052 (+002/051)** — the CVE-2026-42559 class, one POST, fields already recorded, suite already
   sends it: add the check first, prober second.
2. **TRAN-028/062** — "cannot accept → HTTP error" is the robustness MUST every SDK gets wrong somewhere;
   variant (i) needs nothing new.
3. **MRTR-022** — the only probe that reaches stdio parsing; SHOULD, so safe to ship as warnings.
4. **RES-019** (MUST, 2026) — one read, suite-scored, RES-022 already half-covers it.
5. **TRAN-015** — session hygiene; DELETE is recorded; suite sends it.
6. **LOG-003 / PROM-007 / PROM-019 / COMP-003 / COMP-008 / RES-008** — one family, one list-first recipe,
   all SHOULD-level; medium FP.
7. **TRAN-091/103/090, TRAN-101** — SEP-2243 servers only; trivial to send, clear verdict.
8. **MRTR-021/023** — suite-scored; needs an interactive tool, so opt-in.
9. **BASE-011/061, CANC-017** — cheap, low stakes.
10. **AUTH PRM group** — high value for protected servers, but a trace-format and ADR-0022 change.
11. **TRAN-031/034/036/042/043/044/047/067** — recorder work, mostly `2025-11-25`-only SSE resumability,
    which `2026-07-28` removed.
12. **LIFE-012, BASE-063** — marginal.

## 3. Overlap with the official suite (0.2.0-alpha.12)

Scored server scenarios (`package/requirements/*.yaml`, `server:` blocks): 30 at `2025-11-25` (+3
not scored: `server-session-lifecycle` added-after-release, `json-schema-2020-12` and `server-sse-polling`
pending) and 37 at `2026-07-28` (+13 not scored: ten `tasks-*` extension scenarios, `json-schema-2020-12`,
`http-header-validation` and `http-custom-header-server-validation` pending). Of the 81 scenario classes in
the bundle (`scenarios.py`), the server ones that send a *hostile or malformed* stimulus are:

| Suite scenario (status) | What it sends | (a) candidates it already exercises |
|---|---|---|
| `dns-rebinding-protection` (scored, both revisions) | `Host: evil.example.com`, `Origin: http://evil.example.com`, expects 4xx; then the real host, expects 2xx | TRAN-002, TRAN-007, TRAN-051, TRAN-052 |
| `server-session-lifecycle` (2025; not scored) | DELETE with the issued id (`server-session-delete-accepted`), then a request with it (`server-session-terminated-returns-404`) | TRAN-015 |
| `sep-2164-resource-not-found` (scored, 2026) | `resources/read` of `test://nonexistent-resource-for-conformance-testing`; MUST NOT empty `contents`, SHOULD `-32602` with `data.uri` | RES-019 (and the checked RES-022) |
| `input-required-result-validate-input`, `-ignore-extra-params` (scored, 2026) | invalid / extra-key `inputResponses` | MRTR-021, MRTR-023 |
| `input-required-result-tampered-state`, `-unsupported-methods`, `-capability-check`, `-missing-input-response` (scored, 2026) | tampered `requestState`; `InputRequiredResult` on other methods; capability-filtered `inputRequests`; missing keys | MRTR-008 — convicted only through the suite's fixture-tool contract, so (c) here; MRTR-004/012/013/024 are already checked |
| `server-stateless` (scored, 2026) | missing `_meta` fields, unknown/mismatched versions, undeclared capability | already-checked BASE-031/032/035/036, TRAN-073/074, VERS-001 (the existing `--probe` duplicates these) |
| `http-header-validation` (pending) | mismatched/missing `Mcp-Method`, mismatched `Mcp-Name`, whitespace, case | already-checked TRAN-073/097/098/100/102; TRAN-099 stays (c) |
| `http-custom-header-server-validation` (pending) | Base64 values, bad padding, literal without wrapper, omitted header | TRAN-090/091/103 (pending, not scored); checked TRAN-096/136 |

Not in the suite at all (prober-exclusive, field-free): TRAN-028/062, MRTR-022, LOG-003, PROM-007/019,
COMP-003/008, RES-008, TRAN-101, BASE-011/061, BASE-063, CANC-017, LIFE-012 — 18 clauses, 8 MUST-family.
Consequence for scope: do not re-implement G1, G5, G7 or the `server-stateless` family as *probes*;
`mcp-conformance test` already records them. Implement their *checks*, which then also feed the
agreement gate (`03-conformance-strategy.md:157-190`) with the suite's own verdicts on the same traffic.

## 4. Security-relevant subset

Backed by a normative clause and reachable (class a): Origin validation and the 403
(TRAN-002/007/051/052); session termination 404 (TRAN-015); refusing unacceptable input at the HTTP layer
(TRAN-028/062); error shape for malformed JSON and schema-invalid requests (MRTR-022); input-validation
error codes (LOG-003, PROM-007/019, COMP-003/008, RES-008/019); header/body decode-before-compare
(TRAN-090/091/103); authorization discovery (the PRM group, with the field change). Already checked and
merely under-stimulated (a prober adds evidence, not clauses): protocol-version header rejection
(TRAN-020 narrowed to a version the server itself declined, `checks/transport/version.rs:4-29`;
TRAN-073/074/VERS-001 at 2026), `-32021` on undeclared capabilities (BASE-035/036), the challenge shape
(AUTH-045/049/050/051, AUTH-124/129-133: a prober sending `Authorization: Bearer garbage` produces the
`401` these read), SEP-2243 rejections (TRAN-096/098/102/136).

Commonly wanted hardening with **no normative clause** — a registry-driven tool must not report these as
conformance, only as labelled advisories, or not at all:

- `Host` validation / DNS rebinding via `Host`: zero registry quotes name `Host`; the spec's sentence is
  about `Origin`. The everything server validates both as project policy (`05-security-model.md:62-71`),
  and the suite's scenario tests both together, but a Host-only refusal check cites nothing.
- Loopback binding: TRAN-008/053 are SHOULD-level and not wire-visible; a prober could only learn it by
  connecting to a non-loopback address of the same host, which is out-of-band and off-trace.
- Session-id entropy/uniqueness (TRAN-010, SHOULD): not decidable from samples; statistical tests are an
  advisory at best.
- Body size limits, header count/size limits, slowloris, request timeouts: no clause for servers (the 32
  "size/limit" hits are rate-limit, iteration-limit and page-size clauses; timeouts are LIFE-015-family,
  unjudgeable by architecture).
- Rate limiting (TOOL-012/043, COMP-002/011, LOG-004/005, PROG-007): MUST/SHOULD text exists but is
  internal or time-bound — (c), not advisory-able as conformance.
- Stack traces / internal details in `error.message` or `error.data`: the only text is LOG-002/LOG-012,
  about *log messages*, semantic; no clause constrains error-message content.
- CORS headers, TLS on the MCP endpoint, authentication required (TRAN-009/054 SHOULD "proper
  authentication"): no judgeable text. `Origin: null` handling: TRAN-007 covers "present and invalid";
  whether `null` is invalid is the server's policy (the everything server rejects it,
  `policy.rs:107-122`).
- Fuzzing-style malformed JSON beyond the SHOULD of MRTR-022: no MUST exists; a crash is `transport-abort`
  and reads *not observed*.

## 5. Design sketch

**Where.** A `probe` subcommand of `mcp-conformance` (`crates/mcp-conformance-cli`), not a validator
subcommand (ADR-0019 decision 3 keeps the validator free of an async runtime) and not a new crate: the CLI
already owns the orchestration pattern (`run.rs`, `proxy.rs`, `bridge.rs`, `judging.rs`), depends on
`mcp-trace-capture` as a library with its `Recorder` and hyper `http::client` (`capture/http.rs:69-77`,
`Cargo.toml` of the CLI), and is forbidden rmcp/any SDK (`02-architecture.md:57`) — exactly the "built
outside rmcp on purpose" property the host's probe argues for (`probe.rs:16-21`). Move
`mcp_reference_host::probe` (reqwest-based) into it, record directly through the embedded `Recorder`
(no proxy, so `Host` and the raw body are the prober's own to record; one `exchange` id per request is
free), and keep the stdio leg on `stdio::relay`. The reference host's `--probe` is then retired (kept
hidden one release per STABILITY §How a breaking change is made).

**Stimuli as data.** A `probes/<revision>.json` beside the registry: `{id, clauses:[…], breaks:[…],
transport, request:{method, headers, body}}` for static probes (the bytes are the fixture, `probe.rs:20`),
and a closed set of named recipes with parameters for the four dynamic ones (`list-then-ask`,
`delete-then-reuse`, `designated-header-omitted`, `mrtr-retry`). `breaks` names the client-side clauses
the probe violates by construction, which replaces today's hand-written `probe-baseline.json` for users
(the repository keeps its own ledger for its own captures). `cargo xtask spec-drift`-style gates: every
`clauses` id must exist and be server-or-both; every `breaks` id must be client-or-both; each probe must
have one passing and one violating corpus trace (`03-conformance-strategy.md:129-131`).

**Bounded and safe.** Loopback subjects by default; a non-loopback URL needs
`--dangerously-probe-remote` (the project's existing naming, `05-security-model.md:21,29`). No `tools/call`
unless `--tool <name>` names it or the server lists the suite's own fixture tools; never synthesize
arguments for an unnamed tool (the designated-header recipe today calls the host's `run::synthesize_arguments`,
`designated.rs:21` — gate it). Fixed byte strings, no randomisation, no fuzzing loop; ≤ 32 requests,
≤ 64 KiB per body, 10 s per probe (`probe.rs:44-46`), one session, one trace. No Authorization headers
except an operator-supplied invalid bearer for the challenge probes. Never writes outside its work dir.

**Results and exit codes without moving existing verdicts.** Judge the recording with the unchanged
engine. Add a `probe` member to the report (additive, STABILITY.md:39: "Adding a member is not breaking")
modelled on `extensions`: `{expected: [client-side findings matched to a probe's breaks],
unexpected: [everything else]}`. `validate` on the same file keeps reporting its ordinary verdict (it fails,
as the probe capture does today — that is honest), and `probe` exits `0` when `unexpected` is empty, `1`
otherwise, `2` when the run could not be made or judged — the `test` vocabulary (STABILITY.md:37; a new
subcommand is additive under the CLI row). Server findings cite registry ids as today; nothing a probe
learns is reported without one.

**ADR.** Required: an ADR-0024 narrowing "not a security scanner" exactly as ADR-0019 narrowed "not a
gateway" (`0019:32-37`; ADR-0002:104-109 already says "retained as posture": conformance pressure, not
auditing). Its decisions: (1) the prober sends only spec-defined stimuli, each naming a registry clause,
and judges nothing itself; (2) no fuzzing, no vulnerability signatures, no severity, no advisories
labelled as findings — the §4 list is the explicit negative scope; (3) the trace-format additions
(`exchange`, `body`, `sse`, optionally `target_kind`) as optional members, never enum values, with the
STABILITY row updated; (4) the recorder/prober boundary (ADR-0019's "forwards bytes unchanged" is untouched:
the prober is a client, the proxy stays a tap); (5) retroactively records the reference host's `--probe`,
a client that deliberately violates the protocol, shipped against the charter's scanner non-goal
(`00-charter.md:113-114`) with no decision record.

**Recommendation: build a subset, "probe-core", in this order.**
(1) Checks only, no prober code: TRAN-007/052 (+002/051), TRAN-015, RES-019, MRTR-021/023 — the stimuli
are already recorded by `mcp-conformance test`; this also fixes the stale R texts and the PROM-010 (d).
(2) `mcp-conformance probe` = the host's existing ten probes plus G2(i)/G3/G4/G6/G8 — 18 prober-exclusive
clauses across both revisions (8 MUST-family), with the `exchange` and `body` members.
(3) Defer the PRM group (ADR-0022 revisit) and the SSE/stream members (`2025-11-25` resumability, removed at
`2026-07-28`). Do not build advisories, Host-only checks, or anything in class (b).

**Strongest argument against this recommendation.** The arithmetic. 259 of 311 excluded server clauses
(83 %) are unreachable by any client; the prober-exclusive, field-free yield is 18 clauses (5.8 %), ten of
them SHOULD-level that only ever warn, and the highest-value items (Origin 403, session 404, resource
not found, MRTR validation) are checks the project can add this week on recordings it already makes. For
that yield the project takes on a second narrowing of a charter non-goal, a client surface whose every
request must be argued safe, a permanent two-sided baseline ritual (every probe breaks client clauses by
construction, `corpus/README.md:486-495`), and a capability with no calibration partner: the agreement
check works because the suite independently judges the same traffic, and for the 18 prober-exclusive
clauses nobody else does. A maintainer who reads the yield as "warnings for servers that already pass
the suite" is not wrong, and the honest counter is only that TRAN-028/062 and MRTR-022 are the robustness
clauses the suite does not touch and real SDKs do get wrong.

## Appendix A — Re-derivation

- Enumerate: `python3 -I $S/work/probe/enumerate.py /home/user/mcp-conformance/crates/mcp-conformance-core/registry $S/work/probe/out` → `totals.txt`, `excluded-server.{json,tsv}`, `all.json`.
- Classify and total: `python3 -I $S/work/probe/classify.py $S/work/probe/out` → `classification.json`, `classification-totals.txt` (asserts all 311 classified, none extra).
- Suite: `cd $S/work/probe/suite-pkg && npm pack @modelcontextprotocol/conformance@0.2.0-alpha.12 && tar xzf *.tgz -C extracted`; `python3 -I $S/work/probe/scenarios.py extracted/package/dist/index.js $S/work/probe/out/scenarios.tsv` (81 classes; `A` = `2026-07-28`); scored lists from `extracted/package/requirements/{2025-11-25,2026-07-28}.yaml` `server:` blocks (30 / 37).
- Registry greps (§4): `python3 -I` over every `registry/<rev>/*.json` quote with `\bHost\b` → 0, `(stack|internal (system )?detail)` → 0, `[Rr]ate.?limit` → 15, `\b(size|limit|large|maximum|bound)` → 32, `[Tt]imeout` → 12.
- Trace facts: `crates/mcp-conformance-core/src/trace.rs:112-149,214-251,388-397,447-466`; `crates/mcp-trace-capture/src/{relay.rs:58-63,90-136,151-220,239-248,271-282,366-414; http.rs:112-135; framing.rs:120-157; stdio.rs:34-55,150-194; headers.rs:21-33}`; validator `context/framing.rs:17-49,258,284-292,371-384`, `context/refusal.rs:41-45`, `checks/stateless/transport/validation.rs:197-239`, `checks/transport/{status.rs:39-138,version.rs:4-29}`.
- Existing prober: `crates/mcp-reference-host/src/{cli.rs:73-78,probe.rs,probe/designated.rs,render.rs:29-46}`, `conformance/probe-baseline.json`, `corpus/README.md:465-579`, `xtask/src/draft_capture.rs:89`.
- Policy texts: `STABILITY.md:35-44`, `docs/plan/00-charter.md:99-115`, `02-architecture.md:50-58,115-155`, `03-conformance-strategy.md:117-131,157-190`, `05-security-model.md:21-30,62-85`, ADR-0002:104-109, ADR-0019:28-47, ADR-0022:29-66,99-114.
