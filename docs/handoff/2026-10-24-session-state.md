<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Session handoff — 2026-10-24

A working record for the next session, not a plan document: it is dated, it is exempt from the
living-document gates, and [the roadmap](../plan/06-roadmap.md) still owns milestone status. Delete
it when its items are done or moved there. A machine-readable copy of the checklist is
[`2026-10-24-state.json`](2026-10-24-state.json).

Branch: `claude/focused-thompson-jx6akj` (never merged to `main`; no PR opened). Everything below
that says "pushed" is on that branch at `origin`.

**Updated 2026-10-25.** V1, R1 and the interop refresh (I1) are merged on the branch, which is PR 68
(`tomtom215/mcp-conformance#68`, opened that day; nothing else is merged to `main`). Two more agents' work,
C1 and V2, is finished but **not merged**: it is saved in git bundles under `wip/` with their reports. The
sections "CI on the branch", "Verified on the V1-merged tip", "Verified on the R1-merged tip", "Interop
matrix (I1)", "Finished by agents" and "Next, in order" were rewritten in place; the rest is as of
2026-10-24.

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

**Mutation testing on the pull request.** The `Mutants (PR diff)` workflow runs only on a pull request,
and PR 68 was opened at the end, so its first run (run 103, on `04a88ab`) covered the whole branch
diff against `main` at once and failed 10 of its 16 shards: 21 surviving mutants, across the validator,
capture and workbench code. `f8f1ca0` fixed them: 13 killed by new tests, 6 equivalent mutants excluded
in `.cargo/mutants.toml` with the argument for each, and 2 dead-code removals in capture's `output.rs`.
On `f8f1ca0`, CI run 511 and Mutants run 104 both passed (2026-10-25); the pull request is
`mergeable_state: clean`, with no review yet. Next time, run `cargo mutants --in-diff` locally on the
branch diff before opening the PR, and do not push while a Mutants run is in progress (it cancels).

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
   closes only on a green scheduled run. (CI on the branch is green: CI run 511 and Mutants run 104, on `f8f1ca0`.)
2. The capture README's memory table quotes the agent's *release-build* numbers; mine are debug
   (within about 5%; the stdio object-heavy figure differs by 16 MiB). Re-run
   `crates/mcp-trace-capture/tools/measure-memory.py` in release and reconcile the README.
3. **Maintainer decisions:** is a legacy-era `initialize` inside a `2026-07-28` session held to that
   revision's `_meta` clauses (BASE-030, MRTR-002, TOOL-016, TRAN-071; open `spec-ambiguity` entries in
   the mcpsnoop triage); should the task-related MUSTs (PROG-014/015/016, CANC-020) be judged at a
   revision where the tasks page is out of scope (R1 judged them, abstaining on any task whose creation
   the recording did not see).

## Interop matrix (I1), merged 2026-10-25

An agent refreshed the cross-SDK matrix in a worktree; the lead reviewed it and squash-merged it
(`bcbc17b`). What was decided and checked:

- **The seven-day rule applies to the interop pins.** `.github/dependabot.yml` adopts a release only
  after a week in public. The agent first pinned three two-day-old npm releases (1.32.2, `server`
  2.4.0, `node` 2.1.2); the lead checked the registry times, **sent it back, and the maintainer
  confirmed "stick with the compliant versions"**. Now: TypeScript v1 1.32.1 (with express 5.3.0, zod
  4.6.7: the set Dependabot #67 proposes), TypeScript v2 `server` 2.3.1 / `node` 2.1.1 / `express`
  2.0.2, Python 2.3.0 and Go v1.8.0 (already the latest). **rmcp 3.7.0 is the stated exception**
  (the workspace builds against it for the security fix; `interop/README.md` says so).
- **Independently re-checked by the lead:** the ten packages the npm lock adds or moves, each against
  the registry's publish time (none under seven days); the TypeScript v2 server file's SHA-256 at the
  pinned commit over `raw.githubusercontent.com` (equal to `sdk-pins.env`); that no existing golden
  changed in the diff (only two new ones); `cargo xtask gates` including `sdk-matrix --check` and
  `draft-coverage --check` on the squashed tree; and that the 2.4.0-era trace carried the skills
  extension and the 2.3.1-era one does not. CI's interop runner has Node 22.23.3, which the v2
  packages (Node 20+) need.
- **Re-recorded by the agent, not by the lead:** clean-room `setup.sh`, `run.sh` (5 of 5),
  `matrix.sh` and `sdk-matrix --recaptured`, all cells "same result" bar the new rows.
  `ts-stdio` and `ts-http` are `pass-with-warnings` (`BASE-086`, draft-07 `$schema`), which the
  README had misstated as `pass`; it now says what `run.sh` has always enforced.
- **mcpsnoop re-measured** with the pinned binary: 358 traces; the only addition is the TypeScript v2
  `2026-07-28` capture (ours: pass, no flags), no divergence to triage.
- **Process lessons from this agent.** (1) It sent the maintainer's email address to crates.io in a
  `User-Agent` header, once (crates.io rejects an anonymous UA with 403); nothing in any file, diff or
  commit carries it. Use `mcp-conformance currency check (https://github.com/tomtom215/mcp-conformance)`
  and never personal data. (2) `xtask` bakes its workspace path in at compile time, so agents sharing
  one `CARGO_TARGET_DIR` can run each other's binary: copy it privately and check with `strings`.
  (3) It overwrote a few of the lead's scratchpad files from earlier in the session
  (`msg1.txt` to `msg3.txt`, possibly some logs); nothing in the repository.

## Finished by agents (saved in `wip/`)

Two agents worked in worktrees based on `main`, not on this branch. Both committed before the first
pause; their commits are saved in one git bundle, `wip/wip.bundle`, as branches `wip/V1` and
`wip/R1`, with each agent's own final report: [`wip/README.md`](wip/README.md) (how to fetch from it),
[`V1-final-report.md`](wip/V1-final-report.md), [`R1-final-report.md`](wip/R1-final-report.md).

- **V1 (validator report model): merged 2026-10-25** (party attribution with an opt-in
  `validate --party`; a note for requests unanswered at the end of a trace; one rule for the negotiated
  revision, a **behavior change**; `format: byte` asserted in the wire schema, which the official
  runner does not do). The merge was reviewed and re-verified (above); ADR-0018 and ADR-0020 carry
  amendments, the strategy doc's calibration paragraph and the CHANGELOG are updated. The CLI's use of
  the party view is C1, below (finished, not merged).
- **R1 (2025-11-25 ping, progress, cancellation): merged 2026-10-25.** 27 entries (13 judged by 11
  checks, 14 excluded), IDs PING-001..007, PROG-009..019, CANC-018..026 (no 2026-07-28 ID reused),
  15 corpus traces; the revision is at 348 entries / 102 judged / 246 excluded (the README's registry
  table, verified by a gate). The merge needed one fix to R1's `PING-001` (above); the agent's
  claims were otherwise re-run, not trusted (the section above). CHANGELOG, the README's and book's
  counts are updated. Both agents' branches remain in `wip.bundle` as the record.
- **C1 (`mcp-conformance` CLI fixes): finished, NOT merged.** `wip/C1`, tip `19cb7ea`, six commits on
  `cec63bb`, in `wip/wip-c1.bundle`; report [`C1-final-report.md`](wip/C1-final-report.md). Per the agent,
  all six items are done and each was reproduced first and mutation-checked: `test` judges its subject by
  party (a conforming FastMCP server no longer fails the agreement; the suite's own client's and test
  server's findings are listed, labelled and set aside), a note when the suite asks a server for names it
  does not list, `watch http` takes `--no-preserve-host`, two proxy hints, `serverInfo` from `_meta` in the
  library, one printing of `test`'s report, the `watch` tally. The lead checked the branch mechanically
  (base, scope, clean tree, no personal data) and **did not review the diff or re-run its checks**.
  **To merge:** review the diff; it edits `conformance/wire-findings.json` (removes the two `client` entries
  the party view now sets aside) and `mcp-trace-capture` (additive `HostHeader::from_flags`), so re-run
  `cargo xtask conformance` on a quiet machine; the workbench now names the server of
  `sdk-rust-2026-07-28`, so the `visual` baselines for the library and dashboards will change (take them
  from the CI runner's artifact and look at them); CHANGELOG entries (its report has them; one is a
  **behaviour change**: `sessions.verdict` in `test`'s JSON is now party-scoped); consider an ADR for the
  party semantics (a baseline entry explains either end; a held-against-nobody finding counts). The agent
  did not run `falsify-gates`, `cargo doc`, `cargo test` on 1.88, or the workspace-wide clippy and test
  variants.
- **V2 (a baseline file and filters for `validate`): finished in part, NOT merged.** `wip/V2`, tip
  `397b5c0`, five commits on `cec63bb`, in `wip/wip-v2.bundle`; report
  [`V2-final-report.md`](wip/V2-final-report.md). Per the agent: ADR-0025 (**Proposed**: the maintainer
  decides), `--baseline` / `--write-baseline` / `--allow-stale-baseline` keyed by the SARIF fingerprint,
  accepted findings kept visible in every format (SARIF `suppressions`, JUnit skipped, HTML pill), and
  `--area` / `--level` as human-only display filters; default output and all goldens are unchanged
  (nothing under `corpus/` changed; the lead checked that). **Not done:** grouped repeats (undecided; the
  unfinished draft was lost with the scratchpad) and all docs. **To merge:** merge after C1 (V2 adds public
  fields to `Report` and `Finding`); review the ADR first; write the book pages and crate README; note the
  baseline contract: rewording a finding's detail text changes its fingerprint and reopens entries; the
  agent did not run `cargo +1.88 clippy` or any workspace-wide check.

## Next, in order

1. **Review and merge C1, then V2** (above), each by cherry-picking from its bundle onto a local branch of
   the PR branch, then the full chain with `git add` before `gates`, on a machine where no other build
   shares the target directory; the Action, image and `visual` jobs run or simulated; CHANGELOG entries
   from the reports; new `visual` baselines from the CI artifact. V2's ADR-0025 needs the maintainer's
   decision, and its docs and the grouped-repeats question remain.
3. **`mcp-conformance check`**, per [ADR-0024](../plan/decisions/0024-check-an-exerciser-that-does-only-what-a-client-does.md)
   (**Proposed**: the maintainer must accept it first). A read-only-by-default exerciser, so an
   author with a server and no client gets a clause report in one command.
4. `xtask/src/mcpsnoop.rs` `PINNED` 0.26.1 vs 0.28.0 (needs the binary, a re-measure and a re-triage).
   When 2026-10-30 passes, TypeScript 1.32.2 and `@modelcontextprotocol/server` 2.4.0 (with `node`
   2.1.2) are eligible under the seven-day rule; 2.4.0's conformance server declares the skills
   extension, so re-recording it adds evidence. `@modelcontextprotocol/server-everything` 2.0.1 needs
   Node 24 (`run.sh` pins 2026.8.31).
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

- **Releases are on hold (maintainer, 2026-10-25: "let's hold off on any releases").** No version bump,
  release branch, rehearsal run, tag or publish until the maintainer says otherwise. What a release
  would need is in `RELEASING.md`; what stood between this branch and one on 2026-10-25: the
  version-bump commit (still 0.6.0), a local `cargo xtask semver` read (`cargo-semver-checks` was not
  installed), the unreviewed C1 and V2 work, the owner-only first-time setup for the npm and PyPI
  packages and the container image, and the register-currency gate, whose oldest row (3.16) falls due
  on 2026-11-05 and 34 more on 2026-11-23.

- Accept or reject ADR-0024; decide the prober question.
- Merge the branch; close Dependabot #66 and #67 (the interop pins now include #67's set) and issue #50
  as above; issues #7 and #9 are old.
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
- `cargo-deny` 0.20.2 and `cargo-mutants` 27.1.0 (the version `mutants.yml` pins) are installed
  locally; `cargo-audit` and `mdbook` are not. A `cargo mutants` run builds a private copy of the tree;
  with a 17 GB `target/debug` the disk allowance ran out and 12 mutants were reported "unviable"
  (their logs say "No space left on device"): `rm -rf target/debug` first, and read the per-mutant
  log before believing "unviable".
