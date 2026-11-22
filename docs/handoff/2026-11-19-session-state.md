<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Session handoff — 2026-11-19, last rewritten 2026-11-22

A working record for the next session, not a plan document: it is dated, it is exempt from the
living-document gates, and [the roadmap](../plan/06-roadmap.md) still owns milestone status. Delete
it when its items are done or moved there. A machine-readable copy of the checklist is
[`2026-11-19-state.json`](2026-11-19-state.json). It replaces the 2026-10-24 handoff, whose open
items are all either done below or carried into "Next".

Branch: `claude/busy-cray-9rvb8c`, on `origin`, restarted from `main` at `9842607` (the merge
of pull request 79) on 2026-11-21. The owner's word that day: record `/.well-known/` paths, narrow
`RES-019`, then do the release; and before it, overhaul the README, state every file's licence,
and update every dependency.

## Do this first

State on 2026-11-22, after the `0.7.1` release commit.

1. **`0.7.0` was tagged and never published; `0.7.1` replaces it.** The owner tagged `v0.7.0`
   at `ee9b700` on 2026-11-21 (pull request 80, merged green). Its Release run 44959348017
   failed in `Package and attest` at the first SBOM attestation: `actions/attest` v4.2.2
   takes a `CycloneDX` file only with a `serialNumber`, and the SBOMs here omitted it for
   reproducibility. Every publishing job was skipped; crates.io still had `0.6.0` and no
   GitHub release existed when this was written. The `v0.7.0` tag stays where it is
   (`RELEASING.md`: tags are never moved).
2. **The fix** is in `xtask/src/sbom/serial.rs`: a serial number derived from the document's
   content, which `cargo xtask sbom` now requires. All five real SBOMs pass the action's
   check, the same file without the serial fails it, and two generations are byte-identical.
3. **The owner's steps:** merge the `0.7.1` pull request on green CI, run the rehearsal, tag
   `v0.7.1`, approve the `release` environment. The PyPI pending publisher and the npm
   organisation are set up (2026-11-21); `publish-npm` is still expected to fail once, and
   `RELEASING.md` §npm and PyPI step 2 is the by-hand first publish. Whether `publish`
   succeeds for `mcp-trace-capture` depends on its trusted publisher having been configured
   after the 0.6.0 bootstrap, which only the owner's crates.io settings show.
4. **The release is dated 2026-11-22.** If the tag goes out on another day, change the date in
   `CHANGELOG.md`'s `[0.7.1]` heading, the link to its anchor in `book/src/upgrading.md`, and
   `CITATION.cff`.
5. **Tag-only steps are not rehearsed.** A `workflow_dispatch` run skips every step guarded
   by `github.ref_type == 'tag'`: the attestations, the GitHub release, and the three
   publishes. The SBOM refusal hid there. Of those, the provenance attestation and the image
   attestation ran green on `v0.7.0`; the GitHub release, crates.io, npm and PyPI steps have
   not run for this code at all, so read the `v0.7.1` run job by job.
6. **The README describes the released state** (npm, PyPI, the container image). Those
   sentences become true when the Release run publishes, and not before.

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

Registry now (2026-11-21): `2025-11-25` 123 judged of 348; `2026-07-28` 201 judged of 501.

## Authorization probes

Done: the invalid-token probe (`AUTH-042`, `AUTH-044`, `AUTH-047`, `AUTH-048`, `AUTH-081`,
`AUTH-082` and `AUTH-121`, `AUTH-123`, `AUTH-126`, `AUTH-127`, `AUTH-190`, `AUTH-191`), and, on
2026-11-21 with the owner's decision, the Protected Resource Metadata probes: `AUTH-011`/`AUTH-142`
(a returned document names at least one authorization server) and
`AUTH-006`/`AUTH-010`/`AUTH-091`/`AUTH-141` (the URL a challenge's `resource_metadata` names
serves a JSON object).

How it is built, for whoever touches it next. The recorder keeps a request path only when the
path the server is asked for starts with `/.well-known/` (`trace::records_path`), on the request's
`http` event and on its answer's, with the JSON body as the answer's `document`, not as a
`message` event. `engine::validate` takes such events out before judging the session
(`fetches::split`), because a `GET` with no session id judged as MCP traffic fails six clauses on
a conforming server; `tests/fetch_invariance.rs` inserts a fetch at three places in every corpus
trace and requires that no other row moves. A fetch is tied to a challenge's URL by path alone,
on the premise that a recording is of one server; `check` only fetches a URL on the origin it was
given.

Not judgeable, and why: `AUTH-012`/`AUTH-144` (proving a server offers neither discovery
mechanism needs the MCP endpoint's own path, which stays unrecorded); the audience clauses
(`AUTH-043`, `AUTH-059`, `AUTH-083` and their three counterparts), which need a real token
minted for another audience, which only the operator can supply.

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

## What the release does not claim

- **No screen reader has been run** on the redesigned report or the workbench.
- **`RES-019` is narrowed** (owner's decision, 2026-11-21): it fails only on `-32002`, the
  server's own statement that a resource does not exist. A refusal of a URI under an unlisted
  scheme with another code is no longer reported; `-32602` there is still a pass.
- **The local mutation run was incomplete**; the pull request's run was complete and found
  no survivor (see "Do this first", item 4).

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

- **`check`'s end-to-end tests on Windows.** Its test modules are `#[cfg(unix)]` (their
  stand-ins are `sh` scripts); the HTTP legs would run anywhere and have not been tried there.
- **Register rows 2.13, 3.6 and 3.11** pass ninety days on 2026-11-22. Each is the open or closed
  state of GitHub issues in other repositories, which this session's access could not read (HTTP
  403), so the weekly claims-expire run will name them until someone with access re-reads them.
- **Dependencies inside the seven-day cooldown on 2026-11-21:** rmcp 3.11.0 (published
  2026-11-17), jsonschema 0.62.0 (2026-11-21), and 22 lockfile entries, `clap` 4.6.9 among them.
  `nix` 0.32 was tried and backed out: it adds a second copy beside the 0.31 that rmcp's
  `process-wrap` requires.
- **Truth pass over the plan documents** (`06-roadmap.md`, `04-engineering-standards.md`,
  ADR-0009/0011) from the previous handoff: not started. The charter still describes five
  artifacts and not the front-door binary.
- **`prober`** and the stale exclusion rationales of the previous handoff: untouched.
- **`reuse lint` is not a CI job.** The `spdx` gate checks that every tracked file has a header
  or a `REUSE.toml` entry; the linter itself (6.3.0, compliant on 2026-11-21) is a release-time
  step in `RELEASING.md`.

## Dependabot and issues

- #77 (rmcp-macros 3.10.0), #73 and #74 (jsonschema 0.61.2, workspace and fuzz): their content
  is in the dependency commit of 2026-11-21; they close when it merges.
- #66 (Playwright 1.64.1): its content has been on `main` since pull request 79; it should close
  on Dependabot's next run.
- #71 (weekly claims-expire red): see register rows 2.13, 3.6 and 3.11 under "Not done", which
  will keep it red after 2026-11-22.
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
