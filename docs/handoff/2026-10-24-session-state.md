<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Session handoff — 2026-10-24

A working record for the next session, not a plan document: it is dated, it is exempt from the
living-document gates, and [the roadmap](../plan/06-roadmap.md) still owns milestone status. Delete
it when its items are done or moved there. A machine-readable copy of the checklist is
[`2026-10-24-state.json`](2026-10-24-state.json).

Branch: `claude/focused-thompson-jx6akj` (never merged to `main`; no PR opened). Everything below
that says "pushed" is on that branch at `origin`.

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

**CI on the branch:** run 493 on `7d67852` passed all 34 jobs (Linux/macOS/Windows, 1.99.1 and MSRV
1.88, deny, docs, packages, visual, interop). Commits after it are **not yet CI-tested**.

**Local, on the merged tree before the last docs commits:** `fmt`, `clippy -D warnings`,
`cargo test --workspace --all-features --locked` (1,937 passed, 0 failed, 2 ignored), rustdoc with
`-D warnings`, `cargo xtask gates`, `cargo deny`.

## NOT yet run on the merged tree (do these first)

1. `cargo xtask conformance` and `cargo xtask draft-readiness`: the capture crate changed
   (`Host` default, request handling) after the last green run of both.
2. `cargo xtask falsify-gates` on a clean tree (last run before the merges).
3. Push, then read CI on the new tip. Then dispatch the `Scheduled` workflow with `only=claims`
   (if the input still exists) to see the claims gates go green; **issue #50** closes only on a
   green scheduled run.
4. The capture README's memory table quotes the agent's *release-build* numbers; mine are debug
   (within about 5%; the stdio object-heavy figure differs by 16 MiB). Re-run
   `crates/mcp-trace-capture/tools/measure-memory.py` in release and reconcile the README.

## In flight when the session paused

Two agents were running in worktrees based on `main` (not on this branch). Their work is saved as
patches in `docs/handoff/wip/` **if they committed in time; check that directory**. A patch there
is unmerged, may not pass its tests, and was not reviewed.

- **V1 (validator report model)**, items: (1) `actor` on report rows and `party` on findings
  (clause actor; for `both`, the sender of the event at `seq`; absent when undecidable), in every
  format, plus an opt-in `validate --party`; (2) a report note (no verdict change) listing
  requests unanswered when the trace ended; (3) one rule for the negotiated revision (the version
  the server's `initialize` result names; refuse an unknown one); (4) assert `format: byte`
  (base64) in the wire-schema check, aligned with the suite's ajv. Reason: `mcp-conformance test`
  blames servers for the official suite's own client violations (`LIFE-009`, `LIFE-003`);
  reproduced here from a kept recording (the event at the finding's `seq` is a client-to-server
  `logging/setLevel`).
- **R1 (2025-11-25 ping, progress, cancellation)**: the registry records these pages as
  "unclaimed surface, wire-checkable"; a server sending progress for a token never issued passes
  clean at that revision. Needs the ID convention versus the 2026-07-28 PROG/CANC entries
  (`xtask/src/registry_continuity.rs`).

Merge note for both: golden reports will conflict by construction (both change every golden).
Take either side for `corpus/golden/**`, re-run `cargo xtask bless`, review the diff by class.
V1 was built against jsonschema 0.58; re-check its `byte` format registration against 0.60.

## Next, in order

1. Finish V1 and R1 as above. Then **V2**: a baseline/ignore file for `validate` (accepted
   findings with reasons; stale entries reported), `--level`/`--area` filters, grouped repeats
   (BASE-086 appears once per tool schema).
2. **C1** (`crates/mcp-conformance-cli`, after V1): `test` judges agreement by party; say when a
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
