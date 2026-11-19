<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Session handoff — 2026-11-19

A working record for the next session, not a plan document: it is dated, it is exempt from the
living-document gates, and [the roadmap](../plan/06-roadmap.md) still owns milestone status. Delete
it when its items are done or moved there. A machine-readable copy of the checklist is
[`2026-11-19-state.json`](2026-11-19-state.json). It replaces the 2026-10-24 handoff, whose open
items are all either done below or carried into "Next".

Branch: `claude/busy-cray-9rvb8c`, on `origin`, started from `main` at `8435f7e` (the merge of
PR 68). **No pull request is open** (none was asked for). Releases were on hold by the maintainer's
word of 2026-10-25; this session was asked for a release worth standing behind, prepared everything
short of the release commit, and did not bump the version, tag or publish.

## Do this first

1. Read the CI run on the branch tip (`gh run list --branch claude/busy-cray-9rvb8c`). Every push cancelled the run before it, so no
   complete run exists for any commit but the first (run 536, twelve failures, all addressed
   below); the run on the tip is the first full reading of this branch.
2. Take the `visual` baselines from that run's `visual-diffs` artifact, look at them, commit them
   (the procedure of 2026-10-25: the runner's own `-actual.png`, digest checked). They changed
   because the workbench now names the server of a `2026-07-28` session and reports carry the
   party and baseline markup. Until then the `Visual` job is red, and nothing else should be.
3. Decide the three questions under "Maintainer decisions", then open the pull request. Its
   `Mutants (PR diff)` run will be the first mutation run over this session's code (see
   "Not done").

## What was asked, and what was found

The ask: the project "never lived up to its potential", previous sessions stalled, there has been
no release useful in the real world, and it is out of date; change that.

Read against the evidence, the engine was sound and current work was green at the start
(`cargo test --workspace --all-features`: 2032 passed, 0 failed; `cargo xtask gates` exit 0, both
run on `8435f7e` this session). What was missing was a way for someone with a server to get
anything out of it. Run against the official `@modelcontextprotocol/server-memory` package, the
only one-command path (`mcp-conformance test --command`) took 44 seconds and printed 1,212 lines
ending `verdict: fail`: 20 of 28 scenarios failed for want of the suite's own fixture tools, and
eight `LIFE-009` findings that belonged to the suite's client. Every earlier review had said so
(`docs/reports/project-review-2026-09-24.md`, `dogfood-audit-2026-10-24.md`), ADR-0024 had the
answer written down, and it was waiting on a decision.

## Done and verified

Each line is from a command run in this session.

| Item | How it was verified |
|---|---|
| **`mcp-conformance check`** (ADR-0024): one command from a server to its clause report, read-only by default, bounded, no client, SDK or Node.js | 37 unit tests (one a property test against an independent schema check); 14 end-to-end tests of the binary (everything server in process over HTTP at both revisions; `sh` stand-ins for a server the probe ends, an unjudged revision, silence, a server-initiated request, the budgets). By hand against `server-memory`, `server-filesystem` and `server-everything` from npm over stdio: each judged in about a second, `pass` or `pass-with-warnings` (`BASE-086`) |
| `cargo xtask exerciser` (new; also a leg of `cargo xtask conformance`) | `check --call-all` against the everything server, stdio and HTTP at both revisions: 4 sessions, each exit 0 and each clean under `validate --strict` with no party view, so a slip in the hand-written client fails the build |
| `mcp-conformance validate`, `requirements`, `completions` | The validator's command line moved into `mcp_trace_validator::cli`; the validator's 938 tests passed unchanged after the move; a test runs each under the new name |
| `wip/C1` merged (party-aware `test`, fixtures note, `watch http` Host flags, recorder hints, `serverInfo` from `_meta`) | Cherry-picked (one import conflict, resolved as the union); clippy `-D warnings`; 2066 tests passed; the `server-memory` run then read `agreement: holds` with the eight findings set aside. The agreement and judging diffs were read for design; the other 2,500 lines were not read line by line |
| `wip/V2` merged (`--baseline`, `--write-baseline`, `--area`, `--level`, ADR-0025) with the documentation it lacked | Clean cherry-pick; 2170 tests passed; `check --write-baseline` then `--baseline --strict` run against `server-memory` (18 findings accepted, exit 0; `--strict` alone exit 1); a stale and an expired entry provoked by hand and reported as documented |
| Rust 1.100.0 (pin, both CI matrices, Dockerfile builder by digest, the toolchain falsifier) | clippy `-D warnings` with and without `--all-features`; `cargo +1.88.0 clippy --all-targets --all-features -D warnings` (it was red on the V2 work, which had never been run there; fixed) |
| rmcp 3.7.0 to 3.9.0 (RUSTSEC-2026-0462), rustix 1.1.6 (RUSTSEC-2026-0486), jsonschema 0.60.2 (both lockfiles) | `cargo deny --all-features check`: failed before, `advisories ok, bans ok, licenses ok, sources ok` after. `cargo xtask conformance` exit 0 at 3.9.0 (suite 0.1.16; 30 server and 4 client sessions, zero unexplained divergence; capture, exerciser and product legs green); `draft-readiness` 42 passing / 0 failing per mode, equal to the baseline; `draft-capture` exit 0 |
| RUSTSEC-2026-0543 (chrono unmaintained) | Ignored by id with the reason in `deny.toml`; row `chrono-unmaintained` in `docs/plan/deferrals.json`, review by 2027-02-15 |
| Action and image pins Dependabot proposed (#72, #73, #74, #78) | Applied from the PR diffs; `cargo xtask gates` (`ci-permissions`, `toolchain-pin`) |
| Registry follows the published text: `AUTH-124`, `BASE-041`, `BASE-052`, `RES-018` reworded; skills spec at `specification/beta/skills.mdx`; vendored `2026-07-28` schema at upstream `8a18b4b` | `cargo xtask spec-drift` exit 0: 998 quotes and their anchors, both censuses, both schemas match `main`. Goldens re-blessed and the diff classified (schema link in 363 files, skills path in 37, quotes in 6, 2 HTML pages; no outcome or total moved) |
| Ecosystem register | An agent re-fetched 47 rows (15 date-only, 32 rewritten in place) and added 1.10 and 4.9; rows 1.5i and 3.16 rewritten here from the runner results above. `cargo xtask register-currency --check`: all 82 rows within 90 days |
| README, book chapter "Checking a server", baselines and the party view in "Reading a report", `llms.txt`, descriptions | Book built with mdBook 0.5.4 and the new chapter looked at as a rendered page at 1280 px and 390 px (two over-wide code blocks fixed). Every `mcp-conformance` command the root README, the crate README and the chapter show is held to the parser by a test. The README's example output is a real run |
| What CI found on the first push (run 536) | Windows `unused_mut`; the package check's `--version` comparison; a completions assertion at minimal dependency versions. All three fixed |
| The container image carries `mcp-conformance` | **Not built here** (no Docker daemon); CI's `image` job now asserts the binary |

**The final chain, on the tree this file is committed with** (2026-11-19, Rust 1.100.0):
`cargo test --workspace --all-features --no-fail-fast` ran 2,221 tests: 2,220 passed and one
failed (a gate's list of documents did not name the new chapter); that was fixed and its crate
re-run (xtask, 268 passed), and the three crates whose tests read the README were re-run after
the last README edit (1,463 passed, 0 failed). clippy `-D warnings` with `--all-features`, with
`--no-default-features` and at `+1.88.0`; `cargo doc --workspace --no-deps --all-features` under
`RUSTDOCFLAGS=-D warnings`; `cargo xtask gates`; `cargo xtask falsify-gates` (17 of 17 gates
green on the clean tree and red on their edits); `cargo deny --all-features check`;
`cargo xtask spec-drift`; `cargo xtask register-currency --check`. **Not run:** the 1.88 test
suite, Windows and macOS (CI's), `cargo xtask ci` as one command, `cargo package`,
`cargo xtask semver`, the mutation run.

The old handoff's `wip/` bundles are deleted with it: all four branches they held (`wip/V1`,
`wip/R1`, `wip/C1`, `wip/V2`) are merged, and the commits are in this branch's history. Three
dated reports under `docs/reports/` still name the old handoff's path; they are kept as written.

## Maintainer decisions

1. **ADR-0024 and ADR-0025 are implemented and marked so; accepting them is yours**, and tagging
   the release that ships them is the acceptance. Rejecting either is removing the command or the
   flags before that tag.
2. **The everything server's legacy mode names `2026-07-28` in `server/discover` and does not
   keep it.** `check` against the default mode over HTTP with `--call-all` reports `BASE-037`
   (SHOULD) and fails `LOG-008` (MUST NOT); the official suite's scenarios pass on that mode, which
   is the kind of difference this project exists to show. The fix is one line
   (`ProtocolVersion::known_up_to(&ProtocolVersion::V_2025_11_25)` in
   `supported_protocol_versions`); it was made, found to break two tests and the `draft-readiness`
   legacy leg, all of which encode the dual-era behaviour as deliberate, and reverted.
   Recommendation: make the change, drop the legacy leg (the stateless mode it once estimated the
   distance to has existed since August), re-bless. ADR-0024 "As built" has the detail.
3. **The release.** What stands between this branch and `0.7.0` is the release commit of
   `RELEASING.md` (seven version places, the changelog heading, `SECURITY.md`'s table, the
   "not yet released" wording), `cargo xtask semver` read against the changelog's table
   (`cargo-semver-checks` is not installed here), and the owner-only setup: the npm organisation
   and first publish, the PyPI pending publisher, GHCR visibility. The name
   `mcp-conformance` answered 404 on both npm and PyPI on 2026-11-19 (unregistered).
   `mcp-conformance-cli` stays `publish = false`: the binary ships in the archives, the npm and
   PyPI packages and the image, and publishing the crate needs the one-time token bootstrap.

## Not done

- **Mutation testing of this session's code.** `cargo mutants --in-diff` was not run locally (about
  2,800 new lines in `mcp-conformance-cli`; hours on this machine). The PR's sixteen shards will
  run it. Expect survivors, most likely in `check/session.rs` and `check/wire/` (transport paths
  the tests reach only end to end), and close them the way `f8f1ca0` did.
- **Visual baselines** (above).
- **Register rows 2.13, 3.6 and 3.11** pass ninety days on 2026-11-22. Each is the open or closed
  state of GitHub issues in other repositories, which this session's access could not read.
- **Interop matrix pins.** `interop/rust` still pins rmcp `=3.7.0` (inside RUSTSEC-2026-0462);
  TypeScript 1.32.2, `@modelcontextprotocol/server` 2.4.0 and `node` 2.1.2 are eligible under the
  seven-day rule (Dependabot #76). Moving any of them means re-recording the matrix.
- **Playwright 1.64.1** (Dependabot #66): may move the visual baselines; do it with item 2 above.
- **`mcpsnoop` pin** 0.26.1 against 0.28.0 (needs the binary, a re-measure and a re-triage).
- **Truth pass over the plan documents** (`06-roadmap.md`, `04-engineering-standards.md`,
  ADR-0009/0011) from the previous handoff: not started. The charter still describes five
  artifacts and not the front-door binary.
- **`prober`** and the stale exclusion rationales of the previous handoff: untouched.

## Dependabot and issues

- #72, #73, #74, #78: their content is on this branch; close them when it merges.
- #77 (rmcp-macros 3.9.0): done here with rmcp itself; close.
- #76 (interop npm), #66 (Playwright): open, see "Not done".
- #71 (weekly claims-expire red): the toolchain pin, the register rows and the spec drift it
  it was red for
  (the issue's own table, 2026-11-16) are fixed here, and the deferral and suite-pin gates were
  already green. It closes on a green scheduled run after the merge.
- #7 and #9: old, untouched.

## Observations for whoever plans the next stretch

- **The process still costs more than it returns where it guards prose.** Two register rows were
  single lines of 7.6 KB and 3.2 KB of accumulated restatement; the `Unreleased` changelog section
  was a thousand lines before this session added to it; fifty register rows expire on one day.
  The 2026-09-24 review said so (its D5). What paid for itself this session was the opposite
  kind of gate: `spec-drift` (found four real rewordings and a moved file), the MSRV clippy leg
  (found two defects in merged work), the documented-command test (found a broken example), and
  the validator judging the new client.
- **CI runs on every push to `claude/**` and a new push cancels the run before it.** Push once
  per unit of work and read the run; run 536's twelve failures were found an hour late.
- **`git add -A` stages an agent's worktree** under `.claude/worktrees/` as an embedded
  repository. `.claude/` is in this clone's `.git/info/exclude`; a `.gitignore` entry would make
  that permanent.
- The pinned toolchain and the MSRV toolchain can ask opposite things of one line
  (`#[must_use]` on a function returning `ExitCode`); a message on the attribute satisfies both.

## Environment notes

- `~/.cargo/config.toml` (not in the repository) sets `debug = "line-tables-only"` and no
  incremental builds, as before.
- Installed from crates.io this session: `cargo-deny` 0.20.2, `mdbook` 0.5.4. A prebuilt
  `cargo-deny` downloaded from GitHub was not allowed to run; build from source.
- `cargo-mutants`, `cargo-semver-checks` and `cargo-audit` are not installed. There is no Docker
  daemon.
