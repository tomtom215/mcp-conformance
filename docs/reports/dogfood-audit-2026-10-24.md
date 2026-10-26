<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Outsider dogfood audit of the documented commands on real third-party servers — 2026-10-24

**Provenance.** A dated report, exempt from the living-document gates and kept as written.
It was produced by a model-run audit agent in the session that also wrote
`docs/handoff/2026-10-24-session-state.md`; the VERIFIED / PARTIAL / INFERRED labels are the
agent's own. The handoff says which of its claims were independently reproduced afterwards.
`$S` in commands stands for that session's scratch directory, which no longer exists.

---

# A. Dogfood audit of mcp-conformance (HEAD a64fa94) against real third-party MCP servers

Reviewer stance: outside MCP server author, first contact with the toolkit. Only the frozen **debug** binaries in `$S/bin` were used (`$S` = `$S`). Hardware: 4 vCPU Intel Xeon @ 2.10 GHz (`nproc`=4). All versions report `0.6.0`; the validator "judges protocol revisions: 2025-11-25, 2026-07-28". Everything below is **VERIFIED** (I ran it and saw it) unless marked **INFERRED**. Outputs are quoted verbatim, trimmed only where noted; long absolute paths are shortened to `$S/…`.

Scripts I wrote: `$S/work/dogfood-scripts/` (ts-client.mjs, py-client.py, fastmcp-server.py, goserver/main.go, http-session.sh, broken-server.py, raw-client.py, run-modes.sh, gen-traces.py, gen-http-edits.py, crawl.cjs, ping-server.py). Downloads: `$S/work/dogfood/{node-servers,py-venv,gopath,go-server}`. Traces: `$S/work/dogfood/traces/`, `…/traces/broken/`, `…/synthetic/`, `…/http-edits/`. Reports from `mcp-conformance test`: `$S/work/dogfood/conftest-{everything,fastmcp,go}/`.

---

## 1. Wrong verdicts (priority), each with repro + adjudication

### 1.1 HIGH — `mcp-conformance test` blames the server for the official suite's own client-side violations, reports "agreement: does not hold" and tells the server author to triage
Repro (FastMCP server on PyPI `mcp` 2.3.0, streamable HTTP):
```
$ python -I fastmcp-server.py http 3004 &
$ mcp-conformance test --url http://127.0.0.1:3004/mcp --work-dir $S/work/dogfood/conftest-fastmcp/work   # exit=1, 4.9 s
…
failing clauses: 6 (0 explained, 6 unexplained); stale baseline entries: 0
agreement: does not hold
6 unexplained validator-vs-runner divergence(s):
  LIFE-009 on 002.jsonl: "logging/setLevel" uses the server capability logging, which was not negotiated in this session
  LIFE-009 on 004.jsonl: "completion/complete" uses the server capability completions, which was not negotiated in this session
  LIFE-009 on 011.jsonl: "logging/setLevel" uses the server capability logging, which was not negotiated in this session
  LIFE-009 on 023.jsonl: "resources/subscribe" uses the server capability resources.subscribe, which was not negotiated in this session
  LIFE-009 on 024.jsonl: "resources/subscribe" uses the server capability resources.subscribe, which was not negotiated in this session
  LIFE-003 on 031.jsonl: the server answered initialize here, but no notifications/initialized notification follows in the trace
Triage each (our-bug | suite-bug | spec-ambiguity), fix or file upstream, and record explained ones in a baseline (--divergences)
verdict: fail
```
Same on the Go SDK server (`test --command "$S/work/dogfood/go-server/goserver"`, exit=1, 51 s): `3 unexplained validator-vs-runner divergence(s)`, all LIFE-009 (`completion/complete`, `resources/subscribe` ×2). On `@modelcontextprotocol/server-everything` (declares every capability) there were none.

Adjudication: **the clause readings are CORRECT** (LIFE-009 = "Both parties MUST: Only use capabilities that were successfully negotiated", `requirements LIFE-009` says "binding client and server"), but **the offending party is the suite's client** in every case: it called `logging/setLevel`, `completion/complete`, `resources/subscribe` on servers that declared no such capability (the servers answered `-32601`, which is right), and trace 031 is the suite's `conformance-dns-rebinding-test` client sending `initialize` then closing (`cut -c1-260 …/work/traces/031.jsonl` shows exactly 1 POST, the result, `transport-close`). Nothing in the human, JSON, JUnit or SARIF output names the party: the JSON requirement row is `{"findings":[{"check":"lifecycle.negotiated-capabilities-only","detail":"…","seq":10}],"id":"LIFE-009","level":"MUST","outcome":"fail","source":{…}}` — no `actor`, although `requirements --format json` rows do carry `actor`. A server author running the documented command on a conformant server gets exit 1, "does not hold", and an instruction to triage someone else's bug. This is the single most damaging result of the audit: it makes `test` unusable as a server verdict without hand-writing a `--divergences` baseline.

Same gap seen in my own sessions: `raw-client.py` sent `tools/list` with an invented cursor and the report reads `FAIL PAGE-002 (MUST) seq 18: tools/list cursor "zzz" was never issued as a nextCursor…` — correct, but a server author reading their own report cannot tell it is the client's fault.

### 1.2 HIGH — At 2025-11-25 the registry deliberately omits the ping, progress, cancellation and tasks pages, so MUST violations on them pass clean; nothing user-facing says so
`crates/mcp-conformance-core/registry/2025-11-25/sources.json` (read-only): `out_of_scope: ["architecture","basic/utilities/cancellation","basic/utilities/ping","basic/utilities/progress","basic/utilities/tasks","changelog","index","schema","server"]`. The published pages carry MUSTs (fetched with curl): progress.md line 60 "Progress notifications **MUST** only reference tokens that: …", line 94 "Progress notifications **MUST** stop after completion"; cancellation.md line 32 "Cancellation notifications **MUST** only reference requests that: …", line 35 "The `initialize` request **MUST NOT** be cancelled"; ping.md line 29 "The receiver **MUST** respond promptly with an empty response".
Repro: `$S/work/dogfood/synthetic/fn-progress-unknown-token.jsonl` (server sends `notifications/progress` with `progressToken: "nope"` never issued) and `fn-cancel-unknown-request.jsonl` (`notifications/cancelled` for `requestId: 4242`) → both `verdict: fail` only because of my client's PAGE-002; **no PROG/CANC finding exists at 2025-11-25** (there is no PROG/CANC area in that registry; `requirements --revision 2025-11-25 | grep -i progress` matches nothing). The same misbehaviour live (`MODE=progressnotoken`, `MODE=cancelunknown` through the capture wrapper) → same. The 2026-07-28 registry does have PROG-001…008 / CANC-003/005/011. The README's coverage table and "one row per normative clause" sentence do not mention the out-of-scope pages (INFERRED from reading README.md and getting-started.md: no such statement).

### 1.3 MEDIUM — A server that never answers a request, or exits with a request pending, is reported `pass`
```
MODE=noanswer python -I raw-client.py mcp-trace-capture -o broken/noanswer.jsonl stdio -- python -I broken-server.py
    raw-client: timeout waiting for id 3
    totals: 33 pass, 1 fail, 0 warn, …    (the 1 fail is my client's PAGE-002)
MODE=exitmid …     # server sys.exit(0) right after the initialize result
    trace: open, initialize, result, notifications/initialized, tools/list (id 2), transport-close
    totals: 18 pass, 0 fail, 0 warn, 232 excluded, 0 unsupported, 0 not applicable, 71 not observed
    verdict: pass
```
`validate --all broken/noanswer.jsonl | grep -i "unanswer\|never answer"` prints nothing. The capture wrapper also says only `recorded 6 event(s)` for `exitmid`. Adjudication: ARGUABLE as a clause verdict (JSON-RPC's "MUST reply" is only reachable through the excluded umbrella BASE-012), but a crash-mid-request session labelled `pass` with no note is a false negative in practice.

### 1.4 MEDIUM — Revision selection is inconsistent when the server negotiates a different protocolVersion
- Client asks `2025-11-25`, server answers `"protocolVersion":"2024-11-05"` (`MODE=oldversion`, and `synthetic/fn-server-picks-older-version.jsonl`): judged `revision 2025-11-25 (declared by the trace)`, `revision_source: "declared"`, no note, LIFE-006 passes. The negotiated revision is the server's, which this build does not know.
- Client asks `2024-11-05`, server echoes it (`CLIENT_MODE=protoold`): `error: the trace declares protocol revision(s) 2024-11-05, which no available registry describes … exit 2`.
- Client `2025-11-25`, server `2026-07-28` (`synthetic/rev-mismatch-init.jsonl`): `MCP multi-revision validation — revisions 2025-11-25, 2026-07-28 (declared by the trace)`.
Three different behaviours for one situation; the first silently judges a 2024-11-05 session under 2025-11-25 rules.

### 1.5 MEDIUM — Non-base64 image data in a tool result passes; the same bytes in a prompt result fail PROM-003
`MODE=badimage` (tools/call result contains `{"type":"image","data":"not base64!!","mimeType":"image/png"}`) and `synthetic/fn-image-not-base64.jsonl` → no finding, `wire schema: pass`. The vendored `spec-schema/2025-11-25.schema.json` has `ImageContent.data: {"format":"byte","type":"string"}`; the wire-schema check evidently does not assert `format` (INFERRED from the pass). The 2025-11-25 tools page has no normative "MUST be base64" sentence (fetched; its MUST lines are 36, 199, 212, 336, 508), so this is a schema-format gap, not a missing quote. `blobnotb64` (resources/read) is caught by RES-006, prompts by PROM-003 — tools are the odd one out.

### 1.6 MEDIUM — Error code `0` / `-32603` for an unknown prompt passes on real servers (PROM-007/PROM-019 excluded)
Real: the Python SDK 2.3.0 server answered `prompts/get no-such-prompt` with `{"error":{"code":0,"message":"Unknown prompt: no-such-prompt"}}` at 2025-11-25 (`traces/fastmcp-stdio-ts.jsonl` seq 29) and `{"code":-32603,"message":"Internal server error"}` at 2026-07-28 (`time-stdio.jsonl` seq 26, `fastmcp-stdio-py.jsonl` seq 28); all three sessions `verdict: pass`. The official suite reports the same servers as `MCP error 0: Unknown prompt: test_simple_prompt`. Registry: `PROM-007 — excluded: Whether a failure case actually occurred (the name was invalid…) is server-side ground truth a trace does not carry.` ARGUABLE: when the name is absent from the server's own `prompts/list` answer a few events earlier (no `list_changed` between), the trace does carry that ground truth. Same exclusion shape for RES-008, COMP-003, LOG-003, PROM-019, MRTR-022.

### 1.7 LOW — Wrong `MCP-Protocol-Version` value only warns
`http-edits/http-wrong-protocol-version.jsonl` (header `2024-11-05` after negotiating `2025-11-25`) → `WARN TRAN-018 (SHOULD)`; TRAN-017 (MUST "include the `MCP-Protocol-Version: <protocol-version>` HTTP header on all subsequent requests") passes because a header is present. ARGUABLE registry split: the MUST's placeholder is the negotiated version.

### 1.8 LOW — BASE-086 on every TypeScript-SDK server: correct, but un-actionable and 14–18× repeated
`server-everything` and `server-memory` (both `@modelcontextprotocol/sdk` 1.32.2): `WARN BASE-086 (SHOULD)` once per tool schema (14 and 18 findings), `verdict: pass-with-warnings`, `--strict` → exit 1. Spec text fetched: "4. **Recommendation**: Implementors are RECOMMENDED to use JSON Schema 2020-12." → CORRECT. But the `$schema: draft-07` is emitted by the SDK's schema generator, not the server author; there is no way to acknowledge it short of dropping `--strict`. Under `watch` the same shows as `findings so far: 0 fail, 18 warn` while the final totals read `1 warn` (findings vs clauses; inconsistent counters).

### 1.9 Verdicts I checked and found CORRECT (no action)
- `ping` → HTTP 404 + `-32601` on 2026-07-28 Python server: `PASS TRAN-075` (right; see 3.3 for the proxy's misleading hint).
- `CLIENT_MODE=early` (request between `initialize` result and `initialized`): no finding — correct, the spec only forbids requests before the *response*.
- `fn-tools-declared-but-list-32601` at 2025-11-25: no finding — correct, that revision has no "MUST respond to tools/list" sentence (2026-07-28 has TOOL-020).
- `http-notification-answered-500`, `http-missing-content-type`, `http-foreign-origin-accepted`, `http-server-no-session-id-but-client-sends-one`: pass; each clause involved is excluded with a defensible reason or absent from the spec.
- Accounting honesty on a short session (filesystem server, 17 messages): every `PASS` row is bound to traffic present (e.g. TOOL-010/011 pass because the session carries `structuredContent`/`outputSchema`, 2 and 14 occurrences; the Go session without them reads `NOBS TOOL-010/011`); 6 `N/A` rows are exactly the capability-gated checked clauses for undeclared `resources`/`prompts` (RES-004/006, PROM-003/004/005/008); JUnit `skipped="286"` = 232 excluded + 54 not observed.

---

## 2. Crashes / hangs / panics / wrong exit codes

**No panic, hang or wrong exit code was observed in roughly 150 invocations** (capture, validator, `test`, `watch`, `serve`, `library`). Specifics:

| Input | Result | Verdict |
|---|---|---|
| `mcp-trace-capture -o x stdio -- /nonexistent/server` | `cannot start /nonexistent/server: No such file or directory (os error 2)`, exit 2, no file created | correct |
| existing `-o` without `--force` | `… exists; appending would break the trace's sequence numbers (use --force …, -o …, or {session} …)`, exit 2 | correct |
| server exits 0 / 3 / SIGKILL before traffic | exit 0 / 3 / 137; `the server exited (…) before any message was exchanged; the trace … holds 2 transport event(s) and nothing to validate` | correct |
| HTTP upstream down | client gets `502 Bad Gateway` + `cannot reach the upstream …: Connection refused`; trace holds `transport-abort`; validator exit 2 with a precise message | correct |
| server writes banner / NUL byte / invalid UTF-8 / pretty-printed JSON / CRLF to stdout | recorded as string payloads (`"a\u0000b"`; every trace line valid JSON; validator JSON output valid); `FAIL BASE-008`, `FAIL TRAN-004`; CRLF accepted silently | correct |
| 20 MB tool result (live and offline) | captured; `validate` 0.37 s | ok |
| 70 MB line | `malformed trace: line 8: record is 73400481 bytes, exceeding the 68157440-byte limit` + `hint: … --max-line-bytes 73400481`, exit 3; with the flag 0.96 s | correct |
| empty file / `\n` only / blank line mid-file / trailing blank / BOM / JSON array / bare JSON-RPC / truncated last line / not JSON / invalid UTF-8 / NUL inside a string | exit 2 (empty) or 3, each with the documented message and hint (e.g. the `jq -c '.[]'` hint, "this is a bare JSON-RPC message…") | correct |
| seq duplicate / out of order / negative / 2^63 | exit 3 with `seq N is not greater than the previous event's seq M` (negative: `invalid value: integer -1, expected u64`); seq gaps accepted silently (documented) | correct |
| revision 2025-06-18 / 2099-01-01 | exit 2, `which no available registry describes (supported: …)` + `--revision` hint | correct |
| nonexistent file / directory / `-` with another file / `--max-events 5` / `--registry /etc/hostname` / `--revision banana` / no args / `--bogus` | exit 2/2/2/3/2/2/2/2, all with clear messages (directory gets a glob hint) | correct |
| several traces, one unjudgeable | human: `3 traces: 2 pass, … 1 not judged`, `overall verdict: incomplete`; exit 2; JSON has `unjudged_traces` | matches README |
| `--strict` on pass-with-warnings | `note: --strict — SHOULD-level findings are treated as failures, so this run exits 1 …`, exit 1 | correct |

Timing (debug binaries, 4 vCPU Xeon 2.10 GHz): 200 000-event trace (27 MB): `validate` human 11.8 s, json 11.5 s (96 KB), sarif 11.7 s, html 14.5 s (**110 MB** HTML page). 200 000-event live stdio session: `mcp-trace-capture` wrapper 5.1 s wall; `mcp-conformance watch` 20.1 s wall (the exit waits for the final full-trace pass; the book's "never slows the session" is about forwarding, which I did not measure separately). Note the HTML renderer emits 110 MB for 200k events with no cap or warning — LOW.

LOW: `mcp-conformance library import synthetic/empty.jsonl` → `the file is neither a trace, an mcpsnoop capture nor an Inspector export` (true, but "empty file" would be the useful word).

---

## 3. UX friction and error-message problems

3.1 **MEDIUM — `test` against any server that is not the suite's own fixture server reads as 18–21 FAILUREs with no explanation.** `server-everything` 1.0.0: `28 scenarios, 28 checks: 18 FAILURE, 10 SUCCESS; suite verdict: fail`, with lines like `FAILURE prompts-get-simple: Failed: MCP error -32602: MCP error -32602: Prompt test_simple_prompt not found` and `resources-read-text: … Resource test://static-text not found`. The tool never says that the pinned suite's server scenarios expect the suite's reference fixtures (`test_simple_prompt`, `test://static-text`, …) and that a third-party server cannot pass them; a first-time user concludes their server is broken. Combined with 1.1 the command is not usable as advertised on `main`'s README ("runs the pinned official suite against a server … and whether the two agree").

3.2 **MEDIUM — `--upstream` path semantics differ between docs.** README/getting-started: `--upstream http://localhost:3000` + client `http://127.0.0.1:8080/mcp`; watching.md: `--upstream http://localhost:3000/mcp`. Following both (upstream with `/mcp`, client at `/mcp`) → `Cannot POST /mcp/mcp` (404). The proxy's start-up line does explain the mapping and the 404 hint is right in this case, so it is a docs inconsistency rather than a bug.

3.3 **MEDIUM — The proxy's 404 hint misfires on conformant 2026-07-28 traffic.** FastMCP over HTTP via `mcp-trace-capture http` and via `watch http`: `mcp-trace-capture: the server answered 404 to 1 request(s) — is the endpoint path right? …` — the 404 was the correct `ping → 404 + -32601` (TRAN-075 MUST, judged PASS). A server author will chase a non-existent path problem.

3.4 **MEDIUM — No party or level filter on `validate`/`test`.** `--all` is 619 lines for a 17-message session; there is no `--party server`, `--level must`, or `--area` on `validate` (they exist on `requirements`), and no `--ignore BASE-086`-style suppression short of `--divergences` on `test`.

3.5 **LOW — `library list` and the workbench show no server for any 2026-07-28 session.** `library list`: `fastmcp-http-py  pass  2026-10-24  -`, `fetch-stdio … -`, `time-stdio … -` (and "server not recorded" in the page, "(server not recorded) 0" in the findings-by-server chart) while 2025-11-25 sessions show `mcp-servers/everything 1.0.0`. At 2026-07-28 `serverInfo` lives in the discover result's `_meta["io.modelcontextprotocol/serverInfo"]` (present in these traces), which the library does not read. Also `dogfood-fastmcp ` with a trailing space for an empty version.

3.6 **LOW — `requirements <ID>` defaults to the newest revision.** `mcp-trace-validator requirements BASE-086` → `error: revision 2026-07-28 has no requirement BASE-086; it is a 2025-11-25 requirement: pass --revision 2025-11-25` (exit 2). Good message, but the ID came from a report that already stated the revision; it could just resolve it.

3.7 **LOW — `serve` prints its link on stdout, `watch`/capture report on stderr.** `mcp-conformance serve … > out 2> err` leaves `err` empty and the one-shot link in `out`; the book's watching chapter says "Everything goes to stderr" for `watch`, so I looked in the wrong file for ten minutes.

3.8 **LOW — `test` human report repeats the full BASE-086 block (14–16 lines) for every session**: 252 lines for 28 sessions of one TS-SDK server.

3.9 **LOW — `watch` tally counts findings, totals count clauses** (`findings so far: 0 fail, 18 warn` vs `totals: … 1 warn`).

3.10 **LOW — Mixed-revision probe is handled silently.** The Python 2.x client's `server/discover` probe (carrying a 2026-07-28 `_meta`) answered `-32601`, then `initialize` at 2025-11-25: judged at 2025-11-25 "declared by the trace" with no note; a synthetic trace with 2026-07-28 `_meta` on post-initialize requests is instead judged under both revisions. Defensible, but undocumented.

3.11 README over-promise check: `npx mcp-conformance` / `pip install mcp-conformance` — both names are currently unregistered (`npm view mcp-conformance` → E404; `https://pypi.org/pypi/mcp-conformance/json` → 404). The README says so; fine, but a squatter could take either name before the release.

---

## 4. Missing capabilities a server author would want (ranked by how often I needed them)

1. **Party attribution on every finding** (`actor` in JSON/SARIF/JUnit; `--party server|client` filter on `validate` and `test`; `test` should never count the suite's own client violations as divergences). Needed in all three `test` runs and in every one of my 50 raw-client sessions. (HIGH)
2. **"Unanswered request" / "server exited with N requests pending" note** in the validator and in the capture summary. Needed in `noanswer`, `exitmid`, `fn-request-never-answered`. (MEDIUM)
3. **Suppress/acknowledge known SDK-level findings** (`--ignore BASE-086`, or a baseline file for `validate` like `--divergences` for `test`). Needed on every TypeScript-SDK session with `--strict`. (MEDIUM)
4. **2025-11-25 rows for ping/progress/cancellation (and tasks)**, or at least a line in the report/README that these pages are out of scope at that revision. (HIGH for the claim, MEDIUM for the gap)
5. **`format: byte` (base64) assertion in the wire-schema check**, or TOOL-area image/audio rows mirroring PROM-003/004. (MEDIUM)
6. **Take the negotiated revision from the server's `initialize` result** and say so when it differs from the request (1.4). (MEDIUM)
7. **`test`: say which scenarios assume the suite's fixture server**, and offer a preset of the generic ones (`server-initialize`, `ping`, `tools-list`, `tools-call-error`, `resources-list`, `prompts-list`, transport scenarios). (MEDIUM)
8. **Library/workbench: read 2026-07-28 `serverInfo` from `_meta`.** (LOW)
9. **`validate --level must` / `--area` / a one-line summary mode**, and a cap or warning for the HTML renderer on huge traces. (LOW)
10. **Heuristic judgement of the "standard error code" SHOULDs** when the preceding list response proves the name was unknown (1.6). (LOW)

---

## 5. What genuinely works well

- The Quickstart works as written for every real server I had, both transports, three SDKs: wrap with `mcp-trace-capture -o … stdio -- …`, or `http --upstream …` and point the client at `/mcp`; then `validate`. The capture tool forwards bytes unchanged (SDK clients never noticed it), numbers `{session}` files, refuses to overwrite, records non-JSON/NUL/invalid-UTF-8 lines safely, answers 502 when the upstream is down and records `transport-abort`.
- Every violation I planted that the registry covers was caught at the right clause with the right `seq`: BASE-003/004/005/007/008, LIFE-003/005/009/010, LOG-001, RES-006, TOOL-005/006/007/008/010, PAGE-002/003, TRAN-004/013/017/025/027/029/049, BASE-017 (an inputSchema with `"type":"nonsense"`), and the wire schema for missing `content`/`contents`/`text`/`serverInfo`/`inputSchema`, `nextCursor: 12`, `isError: "yes"`, `title: 5`, `jsonrpc` missing or `"1.0"`, both `result` and `error`, `id: null`.
- Findings quote the clause verbatim and link to the published anchor; every quote I spot-checked (BASE-086, PROM-007, TRAN-017/018, TRAN-025, TRAN-027, LIFE-009) matched the fetched spec text.
- Malformed-trace diagnostics are the best I have seen in a CLI: BOM, blank line (with "trailing blank line" detection), JSON array (with the exact `jq` command), bare JSON-RPC, UTF-8, `seq` order, size limit with the exact `--max-line-bytes` value, directory → glob hint.
- Exit codes matched the documented table in every case tried; multi-trace `incomplete` → 2; `--strict` → 1 with an explanatory note.
- Output formats: JSON, JUnit (well-formed per `xmllint`, `tests`/`skipped` arithmetic consistent), SARIF 2.1.0 (rules with `help.markdown`, `partialFingerprints`, file-relative or absolute `artifactLocation`, `startLine` = trace line), HTML (self-contained, zero `<script>` tags).
- Performance is fine even in debug builds (200k events in ~12 s; 20 MB message in 0.4 s).
- `watch` prints findings live as documented and ends with the full report; `--force`/`-o`/`{session}` behave like the recorder.
- The workbench: 60 views crawled with Playwright/Chromium — library, dashboards, every session view, event inspector, two-session compare, search `verdict:fail req:PAGE-*` (14 results), HTML report download; no 5xx, no page errors, no console errors except my deliberate 404 probes; cookie-less → 401, wrong `Host` → 403, token reuse → 403, `/session/../../etc/passwd` → 404; CSP `default-src 'none'; script-src 'self' …`, `x-frame-options: DENY`. `library import/list/stats/tag/redact/export/prune` all worked; malformed imports were refused with the trace error.
- `not observed` / `not applicable` / `excluded` accounting is honest on short sessions (see 1.9).

---

## 6. Coverage: what was run, and what was not

**Servers × transports × clients recorded with `mcp-trace-capture` and judged with `validate` (human, `--all`, json, junit, sarif, html):**
- `@modelcontextprotocol/server-everything` 1.0.0 (SDK 1.32.2): stdio with TS SDK client → `pass-with-warnings`; stdio with Python SDK 2.3.0 client (`server/discover` probe + legacy fallback) → `pass-with-warnings`; **streamableHttp** (PORT 3001) via `http --upstream http://localhost:3001` with TS client incl. GET stream and DELETE → `pass-with-warnings` (82 events); plus the `/mcp/mcp` misconfiguration run → unjudgeable, exit 2.
- `@modelcontextprotocol/server-memory` 1.0.0, stdio, TS client → `pass-with-warnings`; also recorded through `mcp-conformance watch stdio`.
- `@modelcontextprotocol/server-filesystem` 1.0.0 pointed at an empty dir, stdio, TS client → `pass`.
- PyPI `mcp-server-time` 2026.10.19 and `mcp-server-fetch` 2026.10.19 (uv venv, `mcp` 2.3.0), stdio, Python client (2026-07-28) → `pass`.
- My FastMCP server (`MCPServer`, `mcp` 2.3.0): stdio with Python client (2026-07-28) and with TS client (2025-11-25) → `pass`; streamable HTTP (port 3003) via proxy with Python client → `pass`; also through `mcp-conformance watch http` (port 3006).
- Go SDK v1.8.0 server (built in 22 s after one disk-full retry): stdio with TS client (2025-11-25) and Python client (2026-07-28) → `pass`; streamable HTTP (port 3005) via proxy with TS client → `pass`.
- My deliberately broken raw-JSON-RPC server, 39 server-side modes + 5 client-side modes, each through the stdio wrapper (`$S/work/dogfood/traces/broken/`).
- 58 synthetic/hand-edited stdio traces and 16 hand-edited HTTP traces.

**`mcp-conformance test`:** `--command` server-everything stdio (suite 0.1.16 fetched by npx, 57 s, exit 1, agreement holds); `--url` FastMCP HTTP (exit 1, agreement does not hold — 1.1); `--command` Go stdio (exit 1, agreement does not hold — 1.1). Not run: `--client-command` (out of scope: server author stance), `--divergences`/`--expected-failures`, `--spec-version 2026-07-28`, `--format junit|sarif -o` for `test`.

**`mcp-conformance watch`:** stdio (server-memory; 200k-event synthetic session), http (FastMCP). `--otlp` not run (no collector).

**`mcp-conformance serve` / `library`:** 14-recording throwaway library at `$S/work/dogfood/library`; curl probes; Playwright crawl of 60 views (screenshots in `$S/work/dogfood/serve/shots/`). Not run: `library import --from mcpsnoop|inspector` (no such captures), `--open`, `--dangerously-listen-non-loopback`, "Follow live" in the page.

**Not run / could not run:** `cargo install …` (forbidden to run cargo in the repo; binaries were supplied); `npx mcp-conformance` / `pip install mcp-conformance` (packages not published, confirmed 404 on both registries); the repo's `interop/` scripts (read only, used as reference); `mcp-reference-host` (not needed: SDK clients drove every session); Rust SDK conformance server (not requested). Network: all npm/PyPI/Go-proxy/spec fetches succeeded through the proxy; the only environmental problem was the disk filling up (340 MB free) during the first Go build, fixed by cleaning my own Go cache — not a tool issue.
