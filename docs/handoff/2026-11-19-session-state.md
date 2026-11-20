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

State at the end of 2026-11-20. The owner accepted ADR-0024 and ADR-0025, asked for the release
once it is judged ready, and widened the ask the same day: coverage as near 100% as feasible,
defaults that need no flags, a redesigned report and workbench, a GitHub Marketplace listing.

1. CI on the code tip is green: run 557 (`249932d`) completed `success`, all 34 jobs, among
   them `Visual` on the redesign's baselines, every test and clippy leg on Linux, macOS and
   Windows at 1.100.0 and 1.88, `Conformance`, `Coverage`, `Semver`, the `GitHub Action` job, the
   container image and the informational `Minimal versions` job. Run 556 before it failed two
   jobs, both fixed in `249932d`. The commit that carries this line changes only this file.
2. The visual baselines are taken (`19653fd`, from run 555's artifact); nothing is owed there
   unless the report or workbench changes again.
3. **The `0.7.0` release commit is not made, on purpose.** See "Why not released yet".
4. Pull request 79 is open (asked for on 2026-11-20). Its `Mutants (PR diff)` shards are the
   first full mutation run over this code: read them and close the survivors.
5. **The protected-resource metadata probes wait on a decision** (see "Authorization probes").

## Landed on 2026-11-20

| Work | Commits | How it was checked |
|---|---|---|
| Every server scenario the official suite lists (32, the two it marks pending included); the `test_reconnection` fixture in the everything server | `0a58b86` | `cargo xtask conformance` exit 0: 47 checks, 0 failed |
| Defaults for someone who knows no flag: a bare invocation says where to start; `check` writes `report.html` beside its recording and ends with one plain sentence; `--html-report`; the Action's `html-file` | `0a58b86`, `5135729` | Tests of the binary; a run against `server-filesystem` from npm |
| `check` resumes an event stream a server closes before its answer (SEP-1699) | `0a58b86` | Unit and end-to-end tests; found by the gate the moment the fixture existed |
| 23 clauses moved from excluded to judged, as stimulus-conditional checks | `7b8b359`, `412bc85`, `a16b262`, `aa804d3` (regenerated in `efb1560`, `b10a6d6`) | No existing good, captured or SDK trace fails or warns on any of them; 37 new corpus traces; workspace tests |
| `watch` no longer prints a finding and withdraws it when a first `initialize` is refused | `b10a6d6` | The live-corpus invariant over every trace |
| `check` probes, on by default (`--no-probes`) | `d8c3e25` | End-to-end at both revisions over HTTP; the gate's exerciser leg runs every subject with and without them |
| The report and workbench redesign | `50d329c` | Screenshots viewed (two by the maintainer's session, 56 by the agent); the repository's browser specs run by the agent on this machine's Chromium; **not yet on CI's** |
| 20 of the official suite's 22 `auth/*` client scenarios, through rmcp's OAuth client in the reference host (`--oauth-conformance`, feature `oauth`) | `eb53b40` | `cargo xtask conformance` exit 0; `cargo deny` ok with ten exact-pinned duplicate-version skips |

| The invalid-token probe: 12 authorization clauses judged on the one public `Authorization` value a recorder keeps | the commit titled "the invalid-token probe" | Unit tests; six corpus traces; four end-to-end tests against a token-guarded stand-in server, one asserting the user's credential reaches no file and no output |

Registry now: `2025-11-25` 120 judged of 348; `2026-07-28` 198 judged of 501.

## Authorization probes

Done: the invalid-token probe (`AUTH-042`, `AUTH-044`, `AUTH-047`, `AUTH-048`, `AUTH-081`,
`AUTH-082` and `AUTH-121`, `AUTH-123`, `AUTH-126`, `AUTH-127`, `AUTH-190`, `AUTH-191`).

Not done, and waiting on the owner: the Protected Resource Metadata probes. Judging them needs
two additions to the trace format, an `http` event's request path and the JSON document a
metadata URL returned, and the first reverses a line of the security model ("no request target
... is recorded at all"). The proposal is to record a path only when it starts with
`/.well-known/`, which RFC 8615 reserves for public metadata. With that, six clauses can be judged
soundly: `AUTH-011`/`AUTH-142` (a served document names at least one authorization server) and
`AUTH-006`/`AUTH-010`/`AUTH-091`/`AUTH-141` (the URL a challenge's `resource_metadata` names
serves a document). `AUTH-012`/`AUTH-144` cannot: proving a server offers neither discovery
mechanism needs the MCP endpoint's own path, which stays unrecorded. The audience clauses
(`AUTH-043`, `AUTH-059`, `AUTH-083` and their three counterparts) need a real token minted for
another audience, which only the operator can supply.

## What the classification of the exclusions found

All 566 clauses excluded on the morning of 2026-11-20 were read by five agents against each
clause's quote and the code: 240 have no wire observable, 30 happen between other parties, 131
bind a client and need it put under a scripted server, 41 restate clauses judged under other IDs,
24 need the recorder to capture more (stream identity, SSE `id` and `retry`, stdin close, header
name casing, the request target), and 100 looked judgeable. Of those 100, 35 are now judged; the
agents declined 40 or so of the rest as unsound on a closer reading (each exclusion text now says
why); of the 26 authorization rows, 12 are judged (the invalid-token probe) and the
protected-resource metadata ones wait on a decision (see "Authorization probes"). The per-clause
files are in the session's scratchpad, not the repository.

So the ceiling for a tool that reads the wire is far below every clause. What would move it
next, in order of yield: the authorization probes; a scripted server to put clients under (131
rows); a recorder that keeps stream identity and SSE fields (24 rows).

## Findings about this project's own reference server

- With its pending set, the pre-release `0.2.0-alpha.12` suite reports 117 passing and 22
  failing checks in 13 scenarios against the `2026-07-28` mode (`server-stateless`,
  `json-schema-2020-12`, `http-custom-header-server-validation`, ten `input-required-result-*`),
  and 5 unexplained wire-schema divergences. Measured, not triaged; `draft-readiness` stays on
  the active set (42 passing, 0 failing).
- `check`'s probes draw two SHOULD-level findings, both rmcp's behaviour: an invalid log level
  is refused with `-32601` rather than `-32602` (`LOG-003`), and a non-string cursor is served
  (`PAGE-011`).
- `auth/2025-03-26-oauth-metadata-backcompat` fails in the reference host: rmcp refuses the
  fixture's issuer mismatch. Recorded in `conformance/expected-failures.yaml`.

## Why not released yet

The owner asked for the release when it is adversarially judged ready. It is not, yet:

- About 9,000 lines landed in one day from seven agents. Mutation testing covers the transport
  checks (100 of 100 caught), the OAuth code (72, all caught after a test fix) and a fifth of the
  feature checks; the base and client-feature checks, the probes and the redesign have had none.
- CI has run today's code on all three platforms (run 556), but the mutation job runs only on
  a pull request, and none is open.
- No screen reader has been run on the redesign.
- One judgement in the new checks is worth a second opinion before it ships as a MUST-level
  failure: `RES-019` fails a server that refuses a read under a scheme it names nowhere with a
  code other than `-32602`.

After those: the release commit of `RELEASING.md` step 1, then the owner's steps (merge, tag,
the npm organisation and PyPI publisher, GHCR visibility, and for the Marketplace: accept the
developer agreement and tick "Publish this Action to the GitHub Marketplace" on the release).

## Release pass

Run on 2026-11-20 against the tip of the day.

| Check | Result |
|---|---|
| `cargo xtask semver --unreleased` (cargo-semver-checks 0.51.0, built from crates.io) | Every break it lists is in the changelog's table; three capture types that lost `UnwindSafe` were missing from the row and were added |
| `cargo package` of the five published crates, `cargo deny --all-features check` | exit 0; `advisories ok, bans ok, licenses ok, sources ok` |
| `cargo install --path crates/mcp-conformance-cli --locked` into an empty root, then `check` against `server-memory`, `server-filesystem`, `server-everything` from npm | exit 0 each; `pass-with-warnings`, `pass`, `pass-with-warnings` |
| Hostile inputs to the installed binary | Two defects, both fixed with tests: an unreachable URL was reported as "the server answered HTTP 502" (the recorder's own status); a program that cannot start was named twice |
| CI's minimal-versions job | `completions` panicked inside `clap_complete` 4.6.0 and older; floor raised to 4.6.3 (bisected) |
| `cargo +1.88.0 test --workspace --all-features` | 2233 passed; the 2 failures were tests edited while it ran, and pass on a re-run of that crate |
| Not done | The release commit; a rehearsal run of `release.yml`; the README read as GitHub renders it |

Measured and not acted on: with its pending set (`--suite all`), the pre-release
`0.2.0-alpha.12` suite reports 117 passing and 22 failing checks in 13 scenarios against the
everything server's `2026-07-28` mode (`server-stateless`, `json-schema-2020-12`,
`http-custom-header-server-validation` and ten `input-required-result-*`), with 5 unexplained
wire-schema divergences. `draft-readiness` stays on the active set (42 passing, 0 failing).

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
| The container image carries `mcp-conformance` | **Not built here** (no Docker daemon); CI's `image` job asserts the binary; runs 542 and 543 succeeded as a whole, that job with them |
| What run 541 found | Three Windows jobs: the new `mod check;` in `tests/cli.rs` had been inserted between `#[cfg(unix)]` and the `offline` module it guarded. Both modules are Unix-only now; `check`'s end-to-end tests have not been tried on Windows |
| Visual baselines | The 12 changed screenshots are the runner's own `-actual.png` from run 541's `visual-diffs` artifact (its SHA-256 equals the digest GitHub lists, `4b28e541…99e4`). Looked at: the library (the `2026-07-28` Rust session now names its server), the dashboard (one row fewer in "Findings by server", the unnamed one), and the failing report (its only differing rows are the schema link's commit). One search expectation followed: `server:rust*` now finds both Rust sessions |
| Mutation testing of `check` (`cargo mutants` 27.1.0, twelve files) | Started at 35 survivors and 6 timeouts; ended with none. The last run of every file was on the committed tree: `check.rs`, `arguments.rs`, `headers.rs`, `request.rs` 126 caught, 0 missed, 0 timeouts (a pass cut off by a worker restart after those files); `session.rs`, `session/listing.rs`, `sse.rs`, `summary.rs`, `wire.rs`, `wire/http.rs` 117 caught, 0 missed, 0 timeouts, 15 unviable (exit 0); `wire/stdio.rs` 10 caught, 0 missed, 0 timeouts, 2 unviable (exit 0, run before the last test-only edits to other files) |
| What the mutation work found beyond test gaps | **A defect:** a request the server sends during `initialize` was answered and not counted in the summary (fixed; a hand-written HTTP server in the tests shows it). **A behaviour:** an HTTP server whose stream ended without answering the `server/discover` probe ended the run; `check` now falls back to `initialize` on the same endpoint. **A structure:** the non-Unix variant of a two-function `cfg` pair is compiled on no machine that runs the mutation job, so it could never be killed; it is one function now. **Timeouts:** stand-in tests run with `--timeout 2`, so a mutant that makes the client deaf fails in seconds instead of hanging |

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

- **Mutation testing of the code merged from `wip/C1` and `wip/V2`.** The PR's sixteen shards
  run the whole diff; C1's and V2's agents reported killing the mutants they tried, and nobody has
  run it all. `check`'s own files are done (table above).
- **`check`'s end-to-end tests on Windows.** Both test modules are `#[cfg(unix)]` (their
  stand-ins are `sh` scripts); the HTTP legs would run anywhere and have not been tried there.
- **Register rows 2.13, 3.6 and 3.11** pass ninety days on 2026-11-22. Each is the open or closed
  state of GitHub issues in other repositories, which this session's access could not read.
- **Interop matrix pins.** `interop/rust` still pins rmcp `=3.7.0` (inside RUSTSEC-2026-0462);
  TypeScript 1.32.2, `@modelcontextprotocol/server` 2.4.0 and `node` 2.1.2 are eligible under the
  seven-day rule (Dependabot #76). Moving any of them means re-recording the matrix.
- **Playwright 1.64.1** (Dependabot #66): may move every visual baseline; take them from the
  runner again if it does.
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
- Installed from crates.io this session: `cargo-deny` 0.20.2, `mdbook` 0.5.4, `cargo-mutants`
  27.1.0. A prebuilt
  `cargo-deny` downloaded from GitHub was not allowed to run; build from source.
- `cargo-semver-checks` and `cargo-audit` are not installed. There is no Docker
  daemon.
