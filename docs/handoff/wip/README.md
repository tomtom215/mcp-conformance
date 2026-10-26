<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Agent work saved as git bundles

**First set (2026-10-24; both merged):** two finished agent branches, saved when that session paused, as one git bundle:
[`wip.bundle`](wip.bundle) (about 400 KB, goldens included). A bundle keeps the real commits and
their hashes; it is binary, so the SPDX gate skips it. (Patch files from `git format-patch` were
tried first and rejected by that gate: a patch must begin `From <hash>`, so it cannot carry the
licence header, and the gate's exemptions are argued one by one.)

**Status 2026-10-25: both branches are merged onto the branch (reviewed and re-verified; see the session handoff) and are kept here for reference. `wip/R1` needed one fix on merge: its `PING-001` convicted a server for an HTTP `400` the transport requires.**
Checked: the bundle verifies against `a64fa94` (the base both were made on), and fetching it into a
clone yields the two original branch tips with identical hashes: `wip/V1` at `79803bd` (5 commits)
and `wip/R1` at `73c715f` (4 commits).

| Branch | What | State |
|---|---|---|
| `wip/V1` | Validator report model: `party` on findings and `actor` on rows with an opt-in `validate --party` view; a note for requests unanswered when a trace ends; one rule for the negotiated revision; `format: byte` asserted in the wire schema | merged 2026-10-25 (the fifth commit was completed in the merge); the agent's own account is in [`V1-final-report.md`](V1-final-report.md) |
| `wip/R1` | 2025-11-25 `ping`, `progress` and `cancellation` entered in the registry, with checks, corpus traces and regenerated projections | merged 2026-10-25, with a fix to `PING-001`; the agent's own account is in [`R1-final-report.md`](R1-final-report.md) |

To redo a merge from the bundle (both branches are already in):

```text
git fetch docs/handoff/wip/wip.bundle 'refs/heads/wip/*:refs/remotes/wip/*'
git checkout -b v1 <the branch tip>
git cherry-pick a64fa94..refs/remotes/wip/V1     # conflicts are expected where the tip changed
cargo xtask bless                                 # regenerate corpus/golden/**, then review the diff by class
```

R1 changes every golden, so those files will conflict by construction: take either side, then
re-run `cargo xtask bless` and review the whole diff by class. Run the whole gate chain (`cargo test --workspace
--all-features --locked` **and without `--all-features`**, `clippy -D warnings` in both
configurations, `cargo xtask gates` **after `git add`**, `coverage --check`, `draft-coverage
--check`, `spec-drift`, `falsify-gates`) before trusting either.

## Second set (2026-10-25): C1 and V2, finished and **not merged**

Two more agents' branches, saved when the 2026-10-25 session wrapped up, one bundle each. Both sit on
`cec63bb` (a commit of PR 68's branch), so fetch them into a checkout that has it. Neither has been
reviewed by the lead or run on the branch tip; what each report says it ran is the agent's claim. The lead
checked mechanically that each bundle fetches into a scratch clone to the stated tip, that the base is
`cec63bb`, that the worktree was clean, that every file is in the agent's scope, and that no personal data
is in any diff or commit.

| Branch | Bundle | Tip, size | What | Report |
|---|---|---|---|---|
| `wip/C1` | [`wip-c1.bundle`](wip-c1.bundle) | `19cb7ea`, 6 commits, 38 files | `mcp-conformance test` holds its subject to its own party's findings; a note when a server lacks the suite's fixtures; `watch http` Host flags; proxy hints; `serverInfo` from `_meta` in the library; one printing of the report; the `watch` tally | [`C1-final-report.md`](C1-final-report.md) |
| `wip/V2` | [`wip-v2.bundle`](wip-v2.bundle) | `397b5c0`, 5 commits, 67 files | `validate --baseline` / `--write-baseline` / `--allow-stale-baseline`, `--area` / `--level`, and ADR-0025 (**Proposed**); grouped repeats and docs **not done** | [`V2-final-report.md`](V2-final-report.md) |

```text
git fetch docs/handoff/wip/wip-c1.bundle 'refs/heads/wip/C1:refs/remotes/wip/C1'
git fetch docs/handoff/wip/wip-v2.bundle 'refs/heads/wip/V2:refs/remotes/wip/V2'
git checkout -b merge-c1 <the branch tip>
git cherry-pick cec63bb..refs/remotes/wip/C1
```

Merge C1 first, then V2: V2 adds public fields to the validator's `Report` and `Finding`, so code that
builds those literals (C1's tests may) needs the new fields. Then run the whole chain with `git add` before
`gates`, on a machine where no other build shares the target directory: a shared directory let one agent's
`mcp-conformance` binary replace another's during this session and made a local `conformance` run
untrustworthy. What each merge needs beyond that is in the session handoff.
