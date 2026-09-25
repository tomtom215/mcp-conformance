<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0018: Judge the Revision the Trace Declares, and Ship `2026-07-28` in Every Build

**Date:** 2026-09-24
**Status:** Accepted (replaces the feature-gate condition in [02-architecture.md](../02-architecture.md)
§Protocol-revision strategy and roadmap M5's last DoD line)
**Author:** Tom F.

---

## Context

The `2026-07-28` registry and checks shipped behind the non-default `draft-2026-07-28` feature.
The plan said the gate would drop "only after the final spec text ships, M2.5 completes, and the
official suite's scenarios for it stabilize". The first two conditions hold: the text shipped on
2026-07-28 and has had no normative change since except one wording edit (TRAN-081), and every
in-scope page is entered (272 entries). The third does not: npm `latest` of the official suite is
still `0.1.16`, and `0.2.0` exists only as alphas.

Meanwhile the gate produced wrong verdicts. `cargo install mcp-trace-validator` judged every
trace against `2025-11-25`, so each of the five committed `2026-07-28` captures failed with a false
`LIFE-001`, exit 1, and no note (the note only named revisions the build could judge). The feature
was mentioned nowhere a user would look. See the
[project review](../../reports/project-review-2026-09-24.md), finding C1.

## Decision

1. The `2026-07-28` registry and checks are compiled into every build. `draft-2026-07-28` stays as a
   no-op feature in both published crates so manifests that enabled it keep building.
2. `validate` judges a trace against the revision it declares (`initialize`, per-request `_meta`,
   or the `MCP-Protocol-Version` header). A trace declaring nothing is judged against the newest
   supported revision and says so on stderr. A trace declaring only unsupported revisions is
   refused with exit 2. `--revision` overrides; the report records how the revision was chosen.
3. The suite-stability condition is dropped as a gate on *shipping the registry*. It still gates
   what it actually governs: the blocking agreement check at `2026-07-28`, which waits for a stable
   suite `0.2.0` (deferral `suite-0-2-0-stable-pin-bump`).

## Consequences

### Positive

- A default install gives correct verdicts on the current revision: the three conforming
  captures and both good traces pass with no flags, and the other two captures reproduce their
  committed goldens exactly.
- The "wrong revision" failure mode is closed by construction rather than by a note a reader has
  to notice.

### Negative

- A script that relied on the old default for traces declaring no revision now gets
  `2026-07-28` rules. The CHANGELOG gives the migration (`--revision 2025-11-25`).
- The `2026-07-28` verdicts ship before the official runner that calibrates them is stable. The
  weekly `draft-readiness` ratchet measures against the pinned alpha, but it is not the blocking
  agreement check the `2025-11-25` verdicts have. This is the real cost, and it is why decision 3
  keeps that check on the roadmap.
- The default for an undeclared trace is a judgment call. A capture that dropped a `2025-11-25`
  handshake is judged against `2026-07-28`; the stderr note and `revision_source` make that
  visible, but do not prevent it.

## Alternatives Considered

- **Make the feature a default.** Rejected: `default-features = false` (used to drop the CLI's
  `clap`) would silently remove the current revision from library users.
- **Keep the gate and improve the error message.** Rejected: the default path would still judge
  current-revision traces against the previous revision; a better message on a wrong verdict is
  still a wrong verdict.
- **Refuse traces that declare no revision.** Rejected: fragments and partial captures are
  legitimate inputs, and `--revision` already exists for the case where the default is wrong.
- **Wait for suite `0.2.0`.** Rejected: its release date is unknown, and the registry's
  correctness rests on the published text, not on the runner's release state.
