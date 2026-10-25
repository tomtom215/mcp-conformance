<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Unmerged work in progress

Two finished agent branches, saved when the 2026-10-24 session paused, as one git bundle:
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
