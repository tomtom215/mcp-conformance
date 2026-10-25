<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# C1 final report — `mcp-conformance` CLI: the subject's own findings, fixtures note, `watch http` flags

The agent's own report, saved in substance when the 2026-10-25 session wrapped up. Its claims of
what was run are the agent's; the lead did **not** re-run them or review the diff. What the lead did
check, mechanically: branch `wip/C1` (tip `19cb7ea`) sits on `cec63bb` with six commits and a clean
tree; the only file outside the agent's scope is `conformance/wire-findings.json`, which the agent
discloses; no personal data in any diff or commit. **Not merged into PR 68.** The commits are branch
`wip/C1` in [`wip-c1.bundle`](wip-c1.bundle) (see [`README.md`](README.md); fetch with
`git fetch docs/handoff/wip/wip-c1.bundle 'refs/heads/wip/C1:refs/remotes/wip/C1'` on a checkout that has `cec63bb`).

**Original branch (lost with the container; `wip/C1` in the bundle is the record):**
`worktree-agent-a28c88d36a1caa761`, six commits on `cec63bb`, 38 files, +2945 −220.

## Status

All six items done per the agent; tree clean and committed. VERIFIED means the agent ran it and saw
the output; INFERRED means it did not.

## Setup notes the agent gave

- Its worktree started at `a64fa94`, 53 commits behind the stated tip; it ran `git merge --ff-only
  cec63bb` (no commits of its own existed).
- **xtask and the shared target directory.** Before the lead's warning it ran `cargo xtask conformance`
  twice through the alias. The second run wrote `target/conformance/**` into the **shared**
  `/home/user/mcp-conformance/target/conformance` (after a `remove_dir_all`), replacing the lead's
  regenerable run artifacts. After the warning it used a real `target/` holding only a link to the shared
  `debug`, and ran later xtask tasks from private copies (checked with `strings` for its own worktree
  path). It did not run `bless`, `mcpsnoop`, `sdk-matrix` or coverage regeneration.
- The shared `target/debug/{mcp-conformance, mcp-trace-capture, mcp-everything-server,
  mcp-reference-host, xtask}` are now built from its tree. It compared `mcp-trace-validator` and
  `mcp-conformance-core` with another worktree's and found no differences at that time.

## Item 1: `test` judges the subject by party (commit `1c73659`)

**Reproduced (VERIFIED).** `test --url` against `interop/py-server.py` (FastMCP, `mcp` 2.3.0) exited 1
with "agreement: does not hold" and six unexplained findings (LIFE-009 ×5, LIFE-003 ×1), as the
dogfood audit said. With the reference host as `--client-command`, the old build exited 1 on the
suite's test server's BASE-004 and TRAN-027. An offline stand-in suite whose client calls
`logging/setLevel` reproduced it too (red on the old code).

**Fix.** Every report is viewed for the subject's party (`--url`/`--command`: server;
`--client-command`: client), as `validate --party` does. New library API in `agreement.rs`:
`failures_by_party`, `reconcile_for`, `ValidatorFailure::{party, held_against}`,
`Reconciliation::{set_aside, subject()}`; the old `failures` and `reconcile` are unchanged, so xtask's
gate behaves the same. Findings held against the other end that no baseline entry explains are listed,
labelled as held against "the suite's own client / test server", and set aside (not counted). Findings
held against nobody count, tagged `[unattributed]`. A baseline entry explains a failure of either end and
stays live while it explains any (required: `conformance/agreement-divergences.json` has LIFE-003, which
is the suite's client's). The agreement is the suite's pass/fail plus "no unexplained failure held against
the subject (or nobody), no stale entry".

JSON additions: `agreement.party`, `agreement.divergences.set_aside` (absent when empty, so xtask's
artifact is byte-identical), `sessions.party`, `sessions.findings`; `sessions.verdict` now means the
verdict for the subject's party. JUnit and SARIF follow the view (checked in a test).

**After (VERIFIED).** FastMCP `test --url`: "agreement: holds", the six findings listed under "held
against the suite's own client", exit 1 only because the suite fails 21 scenarios (fixtures). The reference
host as client: exit 0, the two findings under "suite's own test server".

**`conformance/wire-findings.json`:** the two `client` entries (BASE-004, TRAN-027) were removed:
`cargo xtask conformance` went red with "2 entries no longer occur" (they are the suite 0.1.16 test
server's findings, now set aside in a client run). A "Triaged 2026-10-25" sentence in the file's `policy`
records why. `agreement-divergences.json` and `client-agreement-divergences.json` are unchanged.

**Tests** (red first or mutation-killed): library `reconcile_for_sets_aside…`,
`a_baseline_entry_explains_either_ends_failure…`, `a_failing_clause_is_one_failure_for_each_party…`;
binary `the_other_ends_failure_is_set_aside…`, `the_suites_own_ends_findings_are_shown_labelled…`; an
offline end-to-end `the_suites_own_client_violation_is_shown_and_not_held_against_the_server`.

**Mutations** (each turned a named test red; every file restored from a `cp` backup and SHA-256
re-checked): M1 set-aside comparison flipped (4 library tests); M2 set-aside before explained (a library
and a binary test); M3 grouping by party broken; M4 view applied for the wrong party; M5 `subject_party`
flipped; M6 verdict ignores the party view (3 binary tests and the offline test).

**Deliberately not done:** what counts toward `holds()` for wire-schema failures is unchanged (`failures`
has never included them; only the displayed clause verdict does).

## Item 2: fixtures note (same commit)

**Reproduced (VERIFIED).** `test --command` on `@modelcontextprotocol/server-everything` 1.0.0: `28
scenarios, 28 checks: 18 FAILURE, 10 SUCCESS`, no explanation. The suite's failure wording was read from
the run; the pinned bundle uses literal names (`callTool({name:\`test_simple_text\`…`).

**Fix** (`src/fixtures.rs`, `render/note.rs`). For tools, prompts, resources and templates, the run's own
`*/list` answers are collected; names the suite asked for (`tools/call`, `prompts/get`,
`resources/read|subscribe|unsubscribe`) that none of those answers contain are reported, after the
suite's verdict, only when the suite failed a scenario and the subject is a server. It does **not** claim
which scenario's failure follows from a missing name (`tools-call-simple-text` passes on a server lacking
`test_simple_text`). A kind with no list answer, or a listing with `nextCursor`, is not assessed; templates
with RFC 6570 operators count as possibly matching. JSON field `suite.fixtures` (null otherwise). On the
Node server: 12 tools, 4 prompts, 4 resources asked for, in 21 of 28 sessions; an independent Python
recount gives the same. Mutations F1, F2, F3, F5 killed.

## Item 3: `watch http` flags (commit `5e1240d`)

**Reproduced (VERIFIED).** `watch http --no-preserve-host` was "unexpected argument"; the startup line
named the flag anyway, and a loopback upstream silently got the recorder's loopback default.

**Fix.** `watch http` has `--preserve-host` and `--no-preserve-host`, mutually exclusive. The mapping
lives once in `mcp_trace_capture::run::HostHeader::from_flags` (**this edits `mcp-trace-capture`**,
additive; `Mode::host_header` delegates to it). `proxy::is_loopback` was deleted after a 30-host-shape
test showed it equivalent to `mcp_trace_capture::http::is_loopback` except for a case-sensitive miss
(`LOCALHOST`). `test --url` now uses `HostHeader::Auto.preserves(http::is_loopback(..))`.
Test `watch_http_reads_the_hosts_flags_as_the_recorder_does` runs a real `watch http` in front of a
stand-in upstream and checks the `Host` received for default, `--preserve-host`, `--no-preserve-host`.
Mutations H1, C6 killed.

## Item 4: proxy hints (commit `af20383`, in `mcp-trace-capture`)

**Reproduced (VERIFIED).** FastMCP answers `ping` with HTTP 404 and `-32601`; the validator reports
`PASS TRAN-075`, yet the recorder printed "…is the endpoint path right?". With this repo's everything
server and `Host: box.internal`, a 403 got only "1 response(s) had a status outside 2xx"; with
`--no-preserve-host` the same request passed the Host check (then 400 "Session ID is required").

**Fix.** A 404 counts for the path hint only if the response was not the MCP server speaking (not an event
stream, not an object with `"jsonrpc":"2.0"`); a framework's `{"detail":"Not Found"}`, HTML and an empty
body keep the hint. A 403 draws a `--no-preserve-host` suggestion when the client's Host was passed
through and its host name differs from the upstream's (port, case, IPv6 brackets ignored); the wording
says "if that is its Host/Origin check". Tests `a_404_the_mcp_server_sent_is_not_a_wrong_path`,
`a_403_to_a_forwarded_foreign_host_names_the_flag…`, `a_host_is_foreign_when_it_names_another_host…`
(stderr drained to EOF, not slept on: a first version flaked). Mutations C1, C3, C4, C5 killed.

## Item 5: library `serverInfo` at 2026-07-28 (commit `f7585a8`)

**Reproduced (VERIFIED).** `library list` showed `cap1 pass 2026-10-25 -` for a FastMCP `2026-07-28`
session. **Fix** (`workbench/entry.rs`): fall back to `result._meta["io.modelcontextprotocol/serverInfo"]`
and `params._meta["io.modelcontextprotocol/clientInfo"]` after the handshake's names; an empty version no
longer leaves a trailing space (audit 3.5). The client side goes beyond the literal ask. After (VERIFIED):
the session lists as `interop-py-server`. **One existing assertion changed:** `search/tests.rs`,
`prompt:test_simple_prompt server:rust*` now also matches `sdk-rust-2026-07-28` (that fixture asks for the
prompt and names `rust-conformance-server` in `_meta`; it was excluded only because of this bug).
Mutations L1, L2 killed.

## Item 6: `test` output and the `watch` tally (commits `1c73659`, `135f0a8`)

**6a, repeated block (reproduced).** stdout carried `== The official suite ==` (54 lines) and the
agreement; stderr printed the same two blocks again. Fix: with a human or JSON report on stdout, stderr
gets only the verdicts; a report in a file (`-o`), `junit` and `sarif` still get the whole thing on
stderr. One existing assertion that required the duplicate (`a_passing_stdio_server…`) changed; new test
`stderr_does_not_repeat_what_the_report_on_stdout_holds`; mutation D1 killed. **Not reproduced:** the
audit's "BASE-086 block repeated for every session" (it appears once in this run; the 252-line figure is
mostly per-session headers, left alone; within-session grouping is V2's).

**6b, watch tally (reproduced).** `findings so far: 0 fail, 14 warn` sat above `totals: … 1 warn`. The
tally now reads `… 14 warn in 1 clause(s)`, counting clauses once per session, revision and requirement,
and naming the wire schema separately (`in 2 clause(s) and the wire schema`). The book's quoted line and
its test are updated. Tests `many_findings_of_one_clause_…`,
`the_tally_says_how_many_clauses_its_findings_are`; mutation T1 killed.

## Docs (commit `19cb7ea`)

`testing.md`, `watching.md`, `troubleshooting.md`, `workbench.md`, the CLI README and the capture
README. The doc-parsing tests (`every_documented_*`) and the README doctests pass.

## Commits (on `cec63bb`)

- `1c73659` fix(cli): hold test's subject to its own party's findings; note fixtures a server lacks
- `af20383` fix(capture): do not ask about the path for a 404 the MCP server sent; name --no-preserve-host for a refused foreign Host
- `5e1240d` fix(cli): watch http takes --no-preserve-host as the recorder does; one loopback test
- `f7585a8` fix(workbench): name the server and client of a 2026-07-28 session from _meta
- `135f0a8` fix(cli): watch's tally says how many clauses its findings are
- `19cb7ea` docs(book): whose finding it is, the fixtures note, watch http's Host flags and the new hints

## Final checks the agent says it ran (final HEAD `19cb7ea`)

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy -p mcp-conformance-cli -p mcp-trace-capture --all-targets -- -D warnings`, default / `--no-default-features` / `--all-features` | 0 / 0 / 0 |
| MSRV: `cargo +1.88.0 clippy` on the same crates | exit 0 (one pre-existing `unknown lint clippy::unused_async_trait_impl` warning from the workspace lint table) |
| `cargo test -p mcp-conformance-cli -p mcp-trace-capture`, default / `--no-default-features` / `--all-features` | exit 0 each (472 / 194 / 472 passed, 0 failed) |
| `cargo test --workspace --all-features` (once, at the end) | exit 0; 2055 passed, 0 failed, 2 ignored |
| `git add -A`, then `cargo xtask gates` (private copy) | exit 0 |
| `cargo xtask conformance` (private copy, second full run) | exit 0: server gate 30 sessions zero unexplained; client, capture and product legs green |

**Not run by the agent:** `cargo test` on the 1.88 toolchain (only clippy), the `visual` job,
`cargo xtask falsify-gates`, the `--no-default-features` and `--all-features` clippy and test variants for
the whole workspace (only the two touched crates), `cargo doc`, and `cargo mutants` itself (the hand
mutations stand in; the zero-surviving-mutants standard is not shown).

## Things the agent says the lead must handle

1. **The `visual` job's workbench baselines will change.** The fixture library's `sdk-rust-2026-07-28`
   now shows a server in the library page and the dashboards (INFERRED from the list output: that session
   went from `-` to a server name). The agent touched nothing under `visual/`; take new baselines from the
   CI runner's `visual-diffs` artifact.
2. **A new CHANGELOG behaviour change:** `sessions.verdict` in `test`'s JSON is party-scoped; a `test` run
   that used to exit 1 on the suite's own end's findings now exits per the subject; anyone who baselined
   those findings in `--divergences` sees them explained or set aside.
3. **Pre-existing flakiness risk:** capture's other `cli_http.rs` tests use `sleep(100ms)` then `try_iter()`
   on stderr and can flake under load; only the agent's own were fixed.
4. **No ADR was written.** One would fit the party semantics (a baseline entry explains either end; a
   held-against-nobody finding counts).
5. The session handoff's "`mcp-conformance test` does not use the party view yet (that is C1)" line is
   stale once this lands.

## Proposed CHANGELOG paragraphs (the agent's)

- **`test` judges the subject, not the suite's own end (behaviour change).** `mcp-conformance test` holds a
  server (`--url`, `--command`) to the findings the validator holds against the server, and a client
  (`--client-command`) to those held against the client. The suite's own client breaks clauses too (it uses
  `logging/setLevel` on a server that never declared `logging`, it closes a session without
  `notifications/initialized`), and its test server answers a notification with HTTP 200; these are listed
  under "held against the suite's own client / test server", labelled, and set aside rather than counted.
  Findings held against nobody count, tagged `[unattributed]`. A conforming FastMCP server no longer fails
  the agreement; the reference host as a client no longer needs `wire-findings.json` entries for the suite's
  test server. The JSON report gains `agreement.party`, `agreement.divergences.set_aside`, `sessions.party`
  and `sessions.findings`; `sessions.verdict` is now the verdict for the subject's party. A divergence
  baseline still explains a failure of either end.
- **`test` says when the suite asked a server for names it does not list.** When scenarios fail and the
  recorded traffic holds `tools/call`, `prompts/get` or `resources/read` requests for names none of the
  server's own list answers include, the report says so after the suite's verdict and the JSON report
  carries `suite.fixtures`. It claims what the requests show, not which failure follows from a missing name.
- **`test` no longer prints its report twice.** With a human or JSON report on stdout, stderr carries only
  the verdicts; a report in a file, or `junit`/`sarif`, still gets the suite's checks and the agreement whole
  on stderr.
- **`watch http` takes `--no-preserve-host`.** `watch http` and `mcp-trace-capture http` share one mapping of
  `--preserve-host`/`--no-preserve-host` (`HostHeader::from_flags`) and the same defaults. (Rust API:
  `mcp_trace_capture::run::HostHeader::from_flags`.)
- **The recorder's exit remarks are more accurate.** A `404` that carries a JSON-RPC message no longer asks
  "is the endpoint path right?"; a `403` to a request whose passed-through `Host` named another host than the
  server's now suggests `--no-preserve-host`. Both apply to `mcp-trace-capture http` and
  `mcp-conformance watch http`.
- **The library names a 2026-07-28 session's server and client.** The workbench and `library list` read
  `io.modelcontextprotocol/serverInfo` and `io.modelcontextprotocol/clientInfo` from `_meta` when a session
  has no `initialize`; an empty version no longer leaves a trailing space.
- **`watch`'s tally names the clauses its findings are.** `findings so far: 0 fail, 14 warn in 1 clause(s)`,
  beside a report whose totals count clauses (a wire-schema finding is named separately).
