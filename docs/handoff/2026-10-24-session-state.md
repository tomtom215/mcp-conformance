<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Session handoff — 2026-10-24

A working record for the next session, not a plan document: it is dated, it is exempt from the
living-document gates, and [the roadmap](../plan/06-roadmap.md) still owns milestone status. Delete
it when its items are done or moved there. A machine-readable copy of the checklist is
[`2026-10-24-state.json`](2026-10-24-state.json).

Branch: `claude/focused-thompson-jx6akj` (never merged to `main`; no PR opened). Everything below
that says "pushed" is on that branch at `origin`.

**Updated 2026-10-25.** V1 and R1 are both merged on the branch. The sections "CI on the branch",
"Verified on the V1-merged tip", "Verified on the R1-merged tip", "Finished by agents" and "Next, in
order" were rewritten in place; the rest is as of 2026-10-24.

## What was asked, and what was found

The ask: bring the project out of a broken state, update everything (dependencies, language,
tests), and build it out to make MCP servers better and safer. Read against the evidence, the
project was **not broken in its code**: at `a64fa94` the build, 1,900 tests, `clippy -D warnings`
and all offline gates passed. What was red was the supply chain and the calendar (verified in CI
logs and reproduced locally): four RustSec advisories on `rustls`, a stale toolchain pin, register
rows past 90 days, a skills-spec quote drift, one surviving mutant, a mutation sweep that could no
longer finish inside its timeout, and two Dependabot PRs. All of those are fixed (below).

## Done and verified

| Item | How it was verified |
|---|---|
| `rustls` 0.23.46, `rustls-webpki` 0.103.16, `tokio` 1.54.1 (RUSTSEC-2026-0346 to -0349, -0382, -0386) | `cargo deny --all-features check` reproduced the failures on the old lockfile and passes on the new; CI's cargo-deny action passed |
| Rust 1.99.1 pin (pin, both CI matrices, Dockerfile digest) | clippy clean; `toolchain-pin` gate; `falsify-gates` 17/17 (the toolchain falsifier needed a real edit, it would have become a no-op) |
| rmcp 3.5.1 to 3.7.0 (floors and macro shim together) | `cargo xtask conformance` (suite 0.1.16) green: 30 sessions zero unexplained divergence; `draft-readiness` 42/0 on both legs equals baseline; `draft-capture` clean; full tests; deny |
| jsonschema 0.58.6 to 0.60.1 (and the fuzz lockfile) | all goldens byte-identical; wire-schema agreement with the runner unchanged; `cargo check` of `fuzz/` |
| Surviving mutant `numbered.rs:107` | hand-applied mutation turns the new test red (exact error captured); file hash restored |
| Weekly mutation sweep 8 to 16 shards | job durations from the failing run: five shards cut at 180 min, three finished at 125/175/175 |
| `GH_TOKEN` removed from the action's install step | grep: nothing read it |
| Playwright 1.63.0 (Dependabot #66's two files) | its own CI run passed all 34 jobs including Visual |
| Register: 17 rows re-verified, new rows 2.21, 4.5 to 4.8, 5.12 | `register-currency --check`: all 80 rows within 90 days; rows 3.1/3.13 and the GHSA-4432 mechanism read in the 3.5.1/3.7.0 sources, RustSec's database checked directly |
| Skills drift: `SKILL-100`, `skills.directory-cacheable`, `SKILL-001` quote, census 81 | `spec-drift` clean; 4 hand mutations killed; goldens diffed by script |
| Capture hardening (Host default for local upstreams, CONNECT/paths, 408, 0600, symlinks, handle reuse) | each started from a failing test and was mutation-checked by the agent; key code reviewed by hand here; full tests on the merged tree |
| Workbench/OTLP hardening (span content, `/api` `Sec-Fetch-Site` rule, export bounds, session-id redaction, events cache) | same; the `Sec-Fetch-Site` rule and its position before authentication read by hand |
| Memory figures for the capture change | **reproduced here** with the agent's script, frozen old binary vs merged build, debug, 3 runs: proxy 302 to 215 MiB, stdio 531 to 420 (the agent reported 404 for the latter) |

**CI on the branch:** run 501 on `28dac2c` (V1 and R1 merged, with the new visual baselines) passed
all 34 jobs: Linux, macOS and Windows at 1.99.1 and MSRV 1.88, deny, docs, packages, the Action, the
image, visual and interop. Earlier: run 497 (V1 merged) failed 3 jobs, fixed and green in run 498;
run 500 (R1 merged) failed only `Visual`, as expected, fixed in run 501. Local verification of each
merge is recorded in the two sections below.

## Verified on the V1-merged tip, and what is still to do

Run on 2026-10-25 on `50569fc` (V1 merged; the commits after it change documentation and one data
file). Every result below is from a command run in this session, not carried over:

- `cargo xtask gates` exit 0 (run after `git add`, since it reads `git ls-files`).
- `cargo xtask conformance` was **red, then green**. V1's negotiated-revision rule retired two entries
  of `conformance/wire-findings.json` (client `LIFE-018` and `TRAN-017` on `sse-retry`): that
  scenario's server answers a `2025-11-25` proposal with `2025-03-26`, which has no registry, so
  `mcp-conformance test` now lists the session as unjudged (`sessions.unjudged` in its JSON report)
  instead of judging it wrongly. The entries were removed; the re-run exited 0.
- `cargo xtask draft-readiness`: 42 passing, 0 failing at both served revisions, matches the committed
  baseline. `cargo xtask draft-capture` exit 0.
- `cargo xtask falsify-gates`: all 17 gates passed on the clean tree and went red on their edits.
- Before that, on identical code: fmt, clippy in both feature sets, `cargo test --workspace`
  (1989 passed with `--all-features`, 1952 without, 0 failed), rustdoc, and three mutations of the
  `byte` format check, all killed.
- Not run locally on V1's code: the 1.88 (MSRV) build and Windows-target clippy. CI covers both.
- **CI run 497 on the pushed tip `19a508d` failed 3 of 34 jobs, all V1 fallout that the local chain
  could not see because it never ran those jobs**: the shipped Action and the container image judge
  every `corpus/good/*.jsonl` with no flags, and V1's new good trace
  `stdio-server-negotiates-2024-11-05` is refused (exit 2) by the rule V1 added; and the `visual` job's
  8 screenshots of the failing and two-revision reports (V1 added party pills and the
  unanswered-requests note, so the pages are taller). Fixed in the commit after `19a508d`: the CI jobs now
  name the exception and assert the refusal; the 8 baselines are the runner's own `-actual.png` from
  the `visual-diffs` artifact (zip SHA-256 matched the digest CI logged), reviewed as images at the top
  of each. Lesson: a change to the validator's defaults or to the HTML report needs the Action, image
  and `visual` jobs run (or simulated: `action/validate.sh` can be run by hand) before the push.

## Verified on the R1-merged tip

R1's four commits were cherry-picked onto the V1-merged branch and checked on 2026-10-25. Everything
below is from a command run in this session:

- **Merge:** only `corpus/README.md` conflicted (the accounting table and the list of good traces).
  `cargo xtask bless` over the merged tree changed 125 goldens; classified by JSON path, the changes are
  `actor` on R1's new rows (120 files), `party` on its findings, and, in V1's five new `2025-11-25`
  traces, R1's 13 not-observed rows and 14 excluded clauses (pass counts unchanged). The accounting
  table was set to the gate's figures (124 and 237 goldens; 85 and 150 distinct not-observed sets).
- **A false positive in R1's `PING-001`, found by V1's tests and fixed.** `tran-018-declined-version-header-refused-400`
  went from `pass-with-warnings` to `fail`: its server answers a ping carrying an unsupported
  `MCP-Protocol-Version` with HTTP `400`, which the transport requires, and the check convicted it of not
  answering with an empty result. R1's own golden had recorded the fail, and its report's "no pre-existing
  row's outcome moved" missed it because the violations harness asserts only "not pass". An error that
  rode an HTTP error status is now not a ping answer (`TraceContext::http_status`); three unit tests pin
  the cases, and with the abstention disabled the 400/404 test fails. Against the 346 goldens that
  existed before R1, no verdict and no pre-existing row's outcome changes after the fix. Lesson: the
  violations harness cannot see a new failure on an old violation trace; compare verdicts against the
  pre-merge goldens, as this session did.
- `cargo fmt --check`; clippy `-D warnings` in both feature sets; `cargo test --workspace`: 2021 passed
  with `--all-features` and 1984 without, 0 failed, 2 ignored in each; rustdoc `-D warnings`;
  `cargo xtask gates` after `git add`.
- `cargo xtask spec-drift` exit 0 (the `2025-11-25` census matches); `conformance` green with zero
  unexplained divergence (30 and 4 sessions) and the product leg green; `draft-readiness` 42/0 at both
  served revisions, equal to the baseline; `draft-capture` exit 0; `falsify-gates` 17/17 on `937389f`.
- **mcpsnoop:** `cargo xtask mcpsnoop` measured 357 traces (342 + R1's fifteen). R1 made two mappings
  in `rules.json` stale ("the `2025-11-25` registry carries no progress clauses"); both were updated by
  reading the clause texts. One real divergence remains, triaged as an mcpsnoop defect: it warns on the
  conforming task session `good/stdio-task-progress` because it takes the `CreateTaskResult` for the
  request's completion, which the `2025-11-25` tasks page contradicts (PROG-014). Whether to report it
  upstream is the maintainer's call.
- The shipped Action and image jobs, simulated by hand against the 14 good traces: the image's three
  validator commands exit 0, 0 and 2, and `action/validate.sh` under the three Action step
  configurations satisfies the job's own assertions (CI run 500 and 501 then agreed).
- **The `visual` job failed in run 500, as predicted**: R1 changes the totals every `2025-11-25` report
  shows. The 24 new baselines (12 report pages, 12 workbench views) are the runner's own renders from that
  run's artifact, whose SHA-256 equals the digest GitHub lists. Measured against the old baselines, the
  report pages and the workbench session and hostile views keep their size and change only digits; the
  four dashboards grow by three rows (CANC 0/9, PING 1/7, PROG 2/11 in the `2025-11-25` coverage
  chart). Looked at as images: the dashboards chart, a session header, the passing report's tiles.

Still to do, in order:

1. Dispatch the `Scheduled` workflow with `only=claims` (if the input still exists): **issue #50**
   closes only on a green scheduled run. (CI on the branch is green, run 501.)
2. The capture README's memory table quotes the agent's *release-build* numbers; mine are debug
   (within about 5%; the stdio object-heavy figure differs by 16 MiB). Re-run
   `crates/mcp-trace-capture/tools/measure-memory.py` in release and reconcile the README.
3. **Maintainer decisions:** is a legacy-era `initialize` inside a `2026-07-28` session held to that
   revision's `_meta` clauses (BASE-030, MRTR-002, TOOL-016, TRAN-071; open `spec-ambiguity` entries in
   the mcpsnoop triage); should the task-related MUSTs (PROG-014/015/016, CANC-020) be judged at a
   revision where the tasks page is out of scope (R1 judged them, abstaining on any task whose creation
   the recording did not see).

## Finished by agents (saved in `wip/`; both merged)

Two agents worked in worktrees based on `main`, not on this branch. Both committed before the first
pause; their commits are saved in one git bundle, `wip/wip.bundle`, as branches `wip/V1` and
`wip/R1`, with each agent's own final report: [`wip/README.md`](wip/README.md) (how to fetch from it),
[`V1-final-report.md`](wip/V1-final-report.md), [`R1-final-report.md`](wip/R1-final-report.md).

- **V1 (validator report model): merged 2026-10-25** (party attribution with an opt-in
  `validate --party`; a note for requests unanswered at the end of a trace; one rule for the negotiated
  revision, a **behavior change**; `format: byte` asserted in the wire schema, which the official
  runner does not do). The merge was reviewed and re-verified (above); ADR-0018 and ADR-0020 carry
  amendments, the strategy doc's calibration paragraph and the CHANGELOG are updated. Not done:
  `mcp-conformance test` does not use the party view yet (that is C1).
- **R1 (2025-11-25 ping, progress, cancellation): merged 2026-10-25.** 27 entries (13 judged by 11
  checks, 14 excluded), IDs PING-001..007, PROG-009..019, CANC-018..026 (no 2026-07-28 ID reused),
  15 corpus traces; the revision is at 348 entries / 102 judged / 246 excluded (the README's registry
  table, verified by a gate). The merge needed one fix to R1's `PING-001` (above); the agent's
  claims were otherwise re-run, not trusted (the section above). CHANGELOG, the README's and book's
  counts are updated. Both agents' branches remain in `wip.bundle` as the record.

## Next, in order

1. **V2**: a baseline/ignore file for `validate` (accepted findings with reasons; stale entries reported),
   `--level`/`--area` filters, grouped repeats (BASE-086 appears once per tool schema).
2. **C1** (`crates/mcp-conformance-cli`; V1's party view exists, the CLI does not use it yet): `test` judges agreement by party; say when a
   subject is not the suite's fixture server (the Node reference server gets 18 of 28 FAILURE,
   reproduced); `watch http` needs `--no-preserve-host` and the new `HostHeader` (it silently
   gained the loopback default); use `mcp_trace_capture::http::is_loopback` in `proxy.rs`; the
   proxy's 404 hint misfires on a conformant `ping` 404 (`-32601`); add a 403 hint naming
   `--no-preserve-host`; show `serverInfo` from `_meta` at 2026-07-28 in the library; dedupe the
   repeated block in `test` output; `watch` counts findings, totals count clauses.
3. **`mcp-conformance check`**, per [ADR-0024](../plan/decisions/0024-check-an-exerciser-that-does-only-what-a-client-does.md)
   (**Proposed**: the maintainer must accept it first). A read-only-by-default exerciser, so an
   author with a server and no client gets a clause report in one command.
4. **I1** (`interop/`): TS v1 1.32.2 with express/zod per the pin procedure (supersedes
   Dependabot #67), a **TypeScript v2 leg** (`@modelcontextprotocol/server` 2.4.0 serves
   2026-07-28; v1 never will), the Rust leg to rmcp 3.7.0, re-record, regenerate
   `book/src/sdk-matrix.md`. Also `xtask/src/mcpsnoop.rs` `PINNED` 0.26.1 vs 0.28.0.
5. Registry: the dogfood and probe reports flag exclusion rationales that are false today
   (TRAN-028, TRAN-062, TRAN-052, TRAN-006, TRAN-001/TRAN-115; PROM-010 duplicates a checked
   sentence). Convert, with corpus pairs, the passive ones the probe report ranks highest:
   Origin refusal (loopback subjects only; false-positive risk is real), session 404, RES-019,
   MRTR-021/023, and the error-shape SHOULDs that the preceding list proves (the Python SDK
   returns code 0 / -32603 for an unknown prompt).
6. Prober (`probe`): the feasibility report recommends a small subset (malformed input to an HTTP
   error, MRTR-022, the error-shape family, header decoding): 18 clauses the suite does not reach,
   against a second narrowing of the "not a security scanner" non-goal. It needs its own ADR and the
   maintainer's decision; do not start it before that.
7. Truth pass: the README's "one row per normative clause" over-claims at 2025-11-25 until R1
   lands; `docs/plan/06-roadmap.md` (line "TypeScript awaits an SDK line" is cleared; M4's
   RustSec line; this session's work); the dated rmcp SSE dossier is obsolete; ADR-0009/0011
   cite superseded facts; `docs/plan/04-engineering-standards.md` and the roadmap need the
   toolchain/rmcp numbers checked.

## Owner-only actions (not done, not attempted)

- Accept or reject ADR-0024; decide the prober question.
- Merge the branch; close Dependabot #66 and #67 and issue #50 as above; issues #7 and #9 are old.
- Register the npm `mcp-conformance` and PyPI `mcp-conformance` names before release: both
  returned 404 today (squatting risk). Cut a release, set up GHCR visibility.
- Consider filing the six rmcp GHSAs absent from RustSec with the advisory database.
- `register-currency` is a calendar gate by the owner's decision: 50 rows dated 2026-08-24 fall
  due on 2026-11-23, 3.16 on 2026-11-05, 1.5i on 2026-11-16 (the last two need the runner re-run).

## Security review status (agent-run, each finding reproduced by the agent)

The full security report is **not** committed: it carries reproduction commands for items still open. This table is its status; the other three audit reports are in [`docs/reports/`](../reports/).

| # | Finding | Status |
|---|---|---|
| 1 | Recorder memory ~18x per message | mitigated for object-heavy messages, otherwise documented; OPEN |
| 2 | Judging one huge line costs 40 to 70x | documented; events cache added to the workbench; OPEN |
| 3 | Workbench cookie reaches other local servers | OPEN (known, banner and README say so; a page there can no longer drive the API) |
| 4 | Proxy rewrote `Host`, defeating the server's rebinding check | FIXED |
| 5 | `{session}` files reopened by path | FIXED |
| 6 | `--force` followed a symlink | FIXED |
| 7 | Trace files world-readable | FIXED |
| 8 | OTLP carried URI queries and error messages | FIXED |
| 9 | Same-site pages drove the API; unbounded export | FIXED |
| 10 | No body timeout / connection cap | 408 on a stalled body FIXED; no connection cap, OPEN |
| 11 | Token passed to the install step | FIXED |
| 12 | Default redaction left the session id | FIXED (payload secrets stay opt-in, now said plainly) |
| 13-14 | Exotic paths, `CONNECT` | FIXED |
| 15-17 | npx tree unpinned; unbounded trace growth; stdio env inherited | OPEN / documented |

## Reported by agents, not independently reproduced

The dogfood report's findings 1.2 to 1.8 and 3.x beyond what is listed above; the 54-session
robustness table; the probe report's class counts (42 of 311); the upstream report's SDK, tier and
prior-art rows (GitHub pages were read through a summariser; the report marks them PARTIAL).
Reproduced here: the suite's 18/28 fixture failures; the `LIFE-009` attribution; the registry's
own text that ping/progress/cancellation at 2025-11-25 are unclaimed; rmcp 3.7.0 versus 3.5.1 source
for GHSA-4432's mechanism; RustSec's rmcp entries; Rust, crates.io and npm versions.

## Environment notes

- The sandbox disk allowance is about 39 GB and was exhausted twice. A user-level
  `~/.cargo/config.toml` (not in the repo) sets `debug = "line-tables-only"` and no incremental
  builds; it cut a full build from 26 GB to 3.6 GB. `cargo clean` after dependency bumps (stale
  hashed artifacts reach 13 GB). Agent worktrees each build their own `target/` (4 to 5 GB).
- Agent worktrees were created from `main`, not from this branch; expect Cargo.lock/toolchain
  differences and golden conflicts at merge.
- `cargo-deny` 0.20.2 is installed locally; `cargo-audit`, `cargo-mutants`, `mdbook` are not.
