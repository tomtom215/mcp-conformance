<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Draft: withdrawing the everything-server offer (rust-sdk#902)

**Status:** drafted 2026-09-24, **not posted** — posting is the maintainer's action.
**Target:** <https://github.com/modelcontextprotocol/rust-sdk/issues/902>

## Why withdraw

rust-sdk now runs its own in-repo conformance server against the official suite on every
push, for both `2025-11-25` and `2026-07-28`, and reports 100% server conformance on each
(register row 3.10, verified 2026-09-24 at rust-sdk `6677eee`). The gap the offer filled —
38/40 at the time of posting — is closed upstream. Leaving the issue open costs the
maintainers triage attention for nothing.

## Comment text

> Closing the loop on this one: since I opened it, `conformance/` in this repo has reached
> 100% on the official suite for both `2025-11-25` and `2026-07-28`, running on every push —
> so the gap this offer was meant to fill is closed, and I'm withdrawing it. Thanks for the
> fast turnaround on #903 along the way.
>
> The server keeps living at
> [tomtom215/mcp-conformance](https://github.com/tomtom215/mcp-conformance) as the
> calibration subject for an offline, clause-level trace validator. If recorded traces of
> rmcp sessions checked requirement-by-requirement would ever be useful in your CI, I'm happy
> to help wire that up — otherwise nothing further is needed here.

Close the issue as "not planned" after posting, or leave closing to the maintainers if the
author cannot close it.

## After posting

1. Delete the `rust-sdk-902-offer-clock` row from `docs/plan/deferrals.json`.
2. Close tracking issue #9 with a link to the comment.
3. Mark backlog item 1 in `docs/plan/07-ecosystem-engagement.md` closed.
