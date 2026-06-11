<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Risk Register

**Status:** Active
**Last reviewed:** 2026-06-11

---

Reviewed at every milestone gate ([06-roadmap.md](06-roadmap.md) sequencing rule 5). Each
risk names its **trigger** — the observable event that converts it from "monitored" to
"plan changes now". A risk without a trigger is an anxiety, not a register entry.

| # | Risk | Likelihood | Impact | Mitigation (standing) | Trigger → response |
|---|------|-----------|--------|----------------------|--------------------|
| R1 | The 2026-07-28 stateless rework lands materially differently than the RC ([register 1.3, 1.5a–1.5b](01-ecosystem-context.md)). Already observed mid-window (2026-06-11 reconciliation): the announcement under-enumerated the rework (`server/discover`, `subscriptions/listen`, tasks extension, MRTR), and the changelog gained authorization entries plus a DCR deprecation after the RC shipped (PR #2862, 2026-06-05) | Medium | High — state machines and registry entries churn | `draft-2026-07-28` feature gate; `applies` ranges localize changes to data + one state-machine variant ([02-architecture.md](02-architecture.md)) | Final spec text diverges from RC on lifecycle/session semantics → freeze the draft corpus, re-derive registry entries from final text before M2.5 closes; never stabilize 0.x → 1.0 before this settles |
| R2 | The official suite absorbs trace validation or ships its own offline mode | Low–Medium | High — core differentiator overlaps the authority | Upstream-first posture means we'd rather merge than compete; agreement check keeps our engine calibrated and therefore mergeable; trace validation is the maintainers' *stated* long-term method ([register 2.11–2.12](01-ecosystem-context.md)), so convergence is the expected ending, not a surprise | Watch signals: SEP-1627 leaving Draft, the `0.2.0` line shipping trace replay, or a conformance-repo issue naming offline validation → offer the engine/corpus upstream immediately; this repo refocuses on Rust reference implementations |
| R3 | rust-sdk maintainers build an in-tree everything server before M2 lands | Medium | Medium — backlog item 1 evaporates | Engage early (M0 standing workstream: issue participation); build rmcp-idiomatic so converging is cheap | Upstream issue/PR for an everything server appears → pivot from "contribute ours" to "contribute tests, fixtures, and review to theirs"; M2's DoD re-targets our server as an independent cross-check |
| R4 | Official suite 0.2.0 line breaks scenario compatibility ([register 2.4](01-ecosystem-context.md)) | High (it is an alpha line) | Medium | Exact-version pinning; scheduled non-blocking alpha tracking job | Alpha-tracking job red for two consecutive scheduled runs → upgrade spike: adapter or pin-bump PR with scenario diff, before any milestone gate |
| R5 | Crate-name loss in a fast-moving namespace (`mcp-host` went free→33 releases in 5 months; [register 5.8](01-ecosystem-context.md)) | Medium | Low–Medium | M0 DoD includes register-or-defer decision with real `0.1.0` publishes, not squats | A chosen name is taken before M0 closes → fall back per [ADR-0003](decisions/0003-crate-naming.md) alternatives table; never bid on names via placeholder spam |
| R6 | Solo-maintainer bus factor | Certain (it is a fact, not a probability) | High for continuity | Plan corpus + ADRs keep all context in-repo; architecture optimized for smallness; standards make the codebase navigable by strangers | A second regular contributor appears → activate `GOVERNANCE.md` committer path deliberately rather than ad hoc |
| R7 | Scope creep toward SDK/agent-framework territory | Medium (gravity of the space) | High — dilutes the one durable thesis | Non-goals in [00-charter.md](00-charter.md); ADR required for any scope change | Any proposed crate/feature that would make a third party depend on us *instead of* rmcp → automatic ADR with explicit charter-amendment, or rejection |
| R8 | Conformance claims dispute (an implementation contests a published verdict) | Low | High — reputational, the trust asset | Verdicts only with reproducible artifacts (trace + registry version); not-applicable accounting; agreement check against the authority | Dispute filed → reproduce publicly; if our check is wrong, fix + amend report + note in the corpus provenance; if the spec is ambiguous, file upstream and mark the requirement contested |
| R9 | Upstream non-engagement (offers ignored, M4 unreachable as defined) | Medium | Medium — strategy leans on upstream adoption | Backlog ordered small-first to build standing; offers time-boxed | Two substantive offers unanswered for 60+ days → M4 DoD re-scopes to conformance-repo contributions and published artifacts (M5) standing alone; charter success criterion 2 re-evaluated honestly |

## Out-of-register risks deliberately accepted

- **Another community conformance tool gains traction first.** Acceptable: the gap analysis
  ([register §5](01-ecosystem-context.md)) shows no trace-validation overlap today, and if a
  better tool wins, the upstream-first posture means contributing there is success too.
- **The spec's pace makes 100%-pass a moving target.** Accepted as the product's reason to
  exist: a conformance toolkit in a still ecosystem is a museum piece.
