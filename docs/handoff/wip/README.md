<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Unmerged work in progress

Two patch series saved when the 2026-10-24 session paused. Each was made with
`git format-patch a64fa94..<branch>` from an agent's worktree, with `corpus/golden/` left out
(goldens are generated; `cargo xtask bless` recreates them). **Neither has been reviewed by the
session's author, and neither has been run on this branch's tip.** Both were checked only to the
extent that they apply cleanly, in order, to their base `a64fa94` (`git am`): V1 as 5 commits
touching 60 files, R1 as 4 commits touching 40 files.

| Series | What | State |
|---|---|---|
| `V1/` | Validator report model: `party` on findings and `actor` on rows with an opt-in `validate --party` view; a note for requests unanswered when a trace ends; one rule for the negotiated revision; `format: byte` asserted in the wire schema | five commits; the fifth is marked `WIP:`; the agent's own account is in [`V1-final-report.md`](V1-final-report.md) |
| `R1/` | 2025-11-25 `ping`, `progress` and `cancellation` entered in the registry, with checks, corpus traces and regenerated projections | four commits; goldens excluded; the agent's own account is in [`R1-final-report.md`](R1-final-report.md) |

To use a series on the branch tip:

```text
git checkout -b v1 claude/focused-thompson-jx6akj
git am -3 docs/handoff/wip/V1/*.patch      # conflicts are expected where the tip changed
cargo xtask bless                          # regenerate corpus/golden/**, then review the diff by class
```

Order: V1 first (it changes every golden), then R1. V1 was built against `jsonschema` 0.58 and
this branch has 0.60: re-check its `byte` format registration. Run the whole gate chain
(`cargo test --workspace --all-features --locked`, `clippy -D warnings`, `cargo xtask gates`,
`cargo xtask coverage --check`, `cargo xtask draft-coverage --check`, `cargo xtask spec-drift`,
`cargo xtask falsify-gates`) before trusting either.
