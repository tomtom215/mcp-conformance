<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# R1 final report — 2025-11-25 ping, progress, cancellation entered

The agent's own report, saved verbatim in substance when the 2026-10-24 session paused. Its claims of
what was run are the agent's; this session did not re-run them. The patch series is
[`R1/`](R1); goldens are excluded from it (`cargo xtask bless` recreates them).

**Branch (lost with the container; patches are the record):** `worktree-agent-a06259b185c3bcb34`, tree clean, nothing pushed.

**Commits on top of `a64fa94`:**

- `83864af` feat(registry): enter `basic/utilities/{ping,progress,cancellation}` at 2025-11-25 — `registry/2025-11-25/{ping,progress,cancellation}.json`, `sources.json` (pages moved to `in_scope`, censuses ping 1/6/2, progress 8/3/3, cancellation 4/5/1), core `registry.rs` embed list, pinned counts 321 to 348 in `registry/tests.rs` and `set.rs`, `AREA_ORDER` gains PING/PROG/CANC, `xtask/src/spec_drift/tests.rs` in-scope pages 13 to 16.
- `4041fd8` feat(validator): `checks/basic_utilities.rs` + `basic_utilities/{ping,progress,progress/lifetime,cancellation}.rs` with tests, 11 rows in `inventory.rs`, `mod basic_utilities` in `checks/mod.rs`. No other validator file touched.
- `9bcac8c` test(corpus): 3 good + 12 violation traces, ledger rows, all goldens (113 changed, 30 new), exclusion ledger (+14 rows), corpus README accounting table (119 / 246 / 84).
- `73c715f` docs(coverage): README coverage table (348 / 102 judged by 96 checks / 246 excluded) and example totals (`246 excluded … 78 not observed`), book reference pages, `book/src/revisions.md` (348/102/246; example line `246 excluded … 71 not observed`; "add up to 348"), `book/src/sdk-matrix.md`, README "not yet released" bullet.

Commits 1 and 2 are green only together (the inventory invariant binds registry and checks both ways); the head is green.

## Clause decisions (27 entries: 13 judged by 11 checks, 14 excluded)

| ID | Level | Actor | Decision |
|---|---|---|---|
| PING-001 | MUST | both | `ping.empty-result`: an answered ping's result is empty (`_meta` allowed); "promptly" is time (LIFE-015's limit), unanswered pings not judged |
| PING-002 | SHOULD | both | excluded: "periodically" is a rate over time; no interval or session length in a trace |
| PING-003 | SHOULD | both | excluded: configuration surface, no protocol form (LIFE-019) |
| PING-004 | SHOULD | both | excluded: a timeout is a duration inside the sender |
| PING-005 | SHOULD | both | excluded: "excessive" is a rate; no count separates flood from long session |
| PING-006 | SHOULD | both | excluded: antecedent is a deadline expiring; consequence at most a `transport-close` that says not why |
| PING-007 | SHOULD | both | excluded: logging, no wire footprint |
| PROG-009 | MUST | both | `progress.token-string-or-integer` |
| PROG-010 | MUST | both | `progress.token-unique-among-active-requests` (per party, until answered) |
| PROG-011 | MUST | both | `progress.value-strictly-increases` (restarts when a new request takes the token) |
| PROG-012 | SHOULD | both | excluded: message relevance is meaning, not shape (as PROG-004) |
| PROG-013 | MUST | both | `progress.token-issued-in-session` (only when the session's `initialize` is recorded) + `progress.stops-after-completion` |
| PROG-014 | MUST | both | `progress.task-token-unchanged` (task identified by `io.modelcontextprotocol/related-task`) |
| PROG-015 | MUST | both | `progress.task-token-unchanged` |
| PROG-016 | MUST | both | `progress.stops-after-completion`: task ends at terminal status via `tasks/get`/`tasks/cancel` result, `tasks/result` answer, or `notifications/tasks/status` |
| PROG-017 | SHOULD | both | excluded: tracking is bookkeeping; wire consequences are PROG-010/013/016/019 |
| PROG-018 | SHOULD | both | excluded: rate = time |
| PROG-019 | MUST | both | `progress.stops-after-completion` (stdio only; HTTP streams give no sent order) |
| CANC-018 | MUST | both | `cancellation.references-issued-request`: id issued in the same direction, with the `initialize` witness; the "believed in-progress" half is not judged (the race the page names) |
| CANC-019 | MUST NOT | client | `cancellation.initialize-not-cancelled` |
| CANC-020 | MUST | both | `cancellation.task-cancelled-by-request` (request answered with a `CreateTaskResult`, cancelled by notification) |
| CANC-021 | SHOULD | both | `cancellation.no-response-after-cancellation`: stdio, both directions, only after the receiver demonstrably read the cancellation (answered a request sent after it) |
| CANC-022 | SHOULD | both | excluded: ignoring a response is the absence of a reaction; no message defined |
| CANC-023 | MUST | both | excluded: graceful handling is internal; the checks do not convict the race (pinned by `stdio-cancellation`) |
| CANC-024 | SHOULD | both | excluded: logging |
| CANC-025 | SHOULD | client | excluded: UI (strategy rule 2) |
| CANC-026 | SHOULD | both | excluded: ignoring = no reaction; the sender is judged by CANC-018/019 instead |

MAY-family instances (ping 2, progress 3, cancellation 1) are counted in `may_census`, not entered, per the census policy.

## ID allocation (settled by the agent)

IDs are unique across the whole set (`validate_requirements` runs over the union), and STABILITY.md says an ID names one clause forever; the continuity module documents that an unchanged sentence gets a *new* entry at a new revision (BASE-001/BASE-043). So no 2026-07-28 ID is reused: PING-001..007 (new area), PROG-009..019 (after PROG-008), CANC-018..026 (after CANC-017). Sentences identical across revisions: PROG-001/009, 003/011, 005/013, 004/012, 007/018, 008/019, CANC-014/023, 015/024, 016/025, 017/026. The continuity gate keys on (section, quote) and the sections differ (`basic/utilities/*` vs `basic/patterns/*`), so no pair is compared and no JUSTIFIED row is needed. Deliberate divergences: actor `both` instead of `client`/`server` (both parties issue requests at 2025-11-25), and CANC-018 judged where CANC-010 is excluded (the `initialize` session-start witness; precedent PAGE-003/PAGE-011). No 2026-07-28 check or its logic is reused: the stateless checks read one direction.

## Verification the agent says it RAN

- `cargo xtask spec-drift` (network): the three pages 9, 7 and 11 quotes, 0 drifted; `2025-11-25: 222 MUST-family, 146 SHOULD-family, 59 MAY-family keyword(s) across 16 in-scope page(s)`, census matches. It exited non-zero only on the then-pre-existing skills drift (since fixed on this branch by `SKILL-100`).
- `cargo xtask bless`; golden diff reviewed: every 2025-11-25 golden `totals.excluded` 232 to 246 and +13 rows (not-observed, or `pass` for PING-001 on `http-session`, `http-authorization`, `stdio-full-session` and the new traces); no pre-existing row's outcome moved; ledger +14; 9 HTML goldens re-rendered.
- `coverage` and `draft-coverage` (+ `--check`), `sdk-matrix`: in sync. `cargo xtask gates`: green. `cargo test --workspace --all-features --locked`: 59 binaries ok after fixing one xtask pin (13 to 16). Clippy `-D warnings`, `fmt --check`, `cargo doc -D warnings`: clean.
- The validator on each new trace: 3 good pass; each of the 12 violations fails exactly its named clause (shared-check rows aside: prog-019/016 also fail PROG-013/016/019; prog-015 also PROG-014 and PROG-013; canc-021 is a warning).
- 22 hand mutations (a flipped comparison and a widened match per check): every one turned the unit tests red, every file restored with identical SHA-256. Caveat: per-test failing names were not captured (`-q`).

**NOT run:** `cargo xtask falsify-gates` (needs a clean committed tree); `cargo xtask conformance` (needs Node and network; the agent expects no new MUST finding from the everything server or reference host, by reading their progress and cancel behavior, not by running it).

## Statements this makes false or newly true

`sources.json`'s old `out_of_scope` reasons are removed; the README's "one row per normative clause" is now true for these pages; README and `book/src/revisions.md` totals are updated. The 2026-07-28 `CANC-010` exclusion text is untouched and still true for that revision.

## Unsure

Whether the maintainers want the task-related MUSTs (PROG-014/015/016, CANC-020) judged at all, since the tasks page is out of scope at this revision. The agent judged them because a recording carries every element they need; the checks abstain on any task the recording did not see created.

## CHANGELOG paragraph the agent proposed (not yet in `CHANGELOG.md`)

The three `2025-11-25` pages the registry had declined as an "unclaimed surface" (`basic/utilities/ping`, `progress`, `cancellation`) are entered by the census method: 27 entries (PING-001 to 007, PROG-009 to 019, CANC-018 to 026), 13 judged by 11 new checks and 14 carrying a documented exclusion, bringing the revision to 348 entries, 102 judged by 96 checks, 246 excluded. A progress notification for a token no request provided, progress after completion (or after a task's terminal status), a cancellation naming a request its sender never issued, a cancelled `initialize`, a task cancelled by notification, and a ping answered with content now fail at this revision; an answer to a cancelled request warns once the receiver demonstrably read the cancellation. Both directions are judged, since either party issues requests here. Verdicts change for every `2025-11-25` trace's totals (13 new rows, 14 more excluded); 15 corpus traces added.
