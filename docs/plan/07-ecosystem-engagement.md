<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Ecosystem Engagement

**Status:** Active
**Last reviewed:** 2026-06-11

---

## Upstream-first policy

The default home for generically useful work is the official repository
(`modelcontextprotocol/rust-sdk` or `modelcontextprotocol/conformance`); this repo holds
what does not fit upstream, and the burden of proof is on keeping something here. Before
building anything substantial:

1. Search upstream issues/PRs for prior or in-flight work (the rust-sdk already ships
   conformance wiring — [register 3.3](01-ecosystem-context.md); assuming gaps without
   checking is how duplicate work and stepped-on toes happen).
2. If it belongs upstream, open an issue describing the approach *before* writing the PR.
3. If upstream declines, build it here with the decline linked — public provenance of why
   the work lives where it lives.

## Contribution backlog (verified gaps)

Every item is anchored to a register row, so the backlog dies gracefully if a fact changes:

| # | Contribution | Anchor | Shape |
|---|--------------|--------|-------|
| 1 | Everything server into rust-sdk (SEP-1730 appendix asks for one in-repo; none exists) | [2.6, 3.4, 3.10](01-ecosystem-context.md) | Issue → design alignment → PR or fixture adoption from M2 — **offered as [rust-sdk#902](https://github.com/modelcontextprotocol/rust-sdk/issues/902)** (2026-06-11; pre-flight in [#9](https://github.com/tomtom215/mcp-conformance/issues/9)); outcome pending, R9 60-day clock running |
| 2 | `rmcp`-keyed RustSec advisory for CVE-2026-42559 (direct `rmcp < 1.4.0` dependents get no `cargo audit` warning). **Blocked** (2026-06-14): the CVE + rmcp's GHSA are already aliased onto the dynoxide-rs advisory RUSTSEC-2026-0140, so a clean rmcp advisory would collide — needs RustSec-maintainer reconciliation first (issue, not a drop-in PR; draft + analysis in [docs/reports/rmcp-cve-2026-42559-rustsec-advisory-draft.md](../reports/rmcp-cve-2026-42559-rustsec-advisory-draft.md)) | [4.3](01-ecosystem-context.md) | Open a reconciliation issue on rustsec/advisory-db; file the rmcp advisory if maintainers agree |
| 3 | MSRV policy for rust-sdk (none declared; current floor **1.88**, set by the transitive `darling 0.23.0` MSRV — **not** rmcp's own source — register 3.5, empirically re-verified 2026-06-15). Caveat: cargo already surfaces this with a *clear* error naming darling, so the value is Tier-1 MSRV **documentation**, not rescuing users from opaque breakage | [3.5](01-ecosystem-context.md) | Issue proposing rmcp declare `rust-version` + CI job + bump policy; lead with the darling-driven floor and drop the disproved let-chains/E0658 framing ([draft](../reports/rust-sdk-msrv-policy-issue-draft.md)) |
| 4 | Conformance scenarios/fixtures where Rust runs expose suite gaps | [2.3, 3.6](01-ecosystem-context.md) | Small PRs to the conformance repo, SEP-tagged where applicable |
| 5 | 2026-07-28 stateless-rework readiness (test targets before the Tier-1 window — two weeks per SEP text, ~10 weeks observed this cycle) | [1.3, 1.5a, 2.5](01-ecosystem-context.md) | Roadmap M2.5: draft corpus + multi-revision validator support published early; findings filed upstream |
| 6 | Spec-vs-suite discrepancies discovered by the agreement check | [03-conformance-strategy.md](03-conformance-strategy.md) | Issues with reproducing traces attached |
| 7 | SEP-2484 traceability tooling: the registry already stores the `sep-NNNN.yaml` shape, so an emitter plus a completeness checker (every MUST/MUST NOT mapped, exclusions carrying tracking links) is a query over existing data — recurring per-SEP work the gate created with no owning tool | [2.9, 2.11](01-ecosystem-context.md) | Offer upstream as a conformance-repo utility once M1 publishes; design note first |
| 8 | Suite reliability where it scores tiers: tier-check counting bug and SDK-repo lifecycle boilerplate, both open upstream | [2.13](01-ecosystem-context.md) | Small, obviously-correct PRs to the conformance repo — credibility builders shaped like backlog items 2 and 3 |
| 9 | rmcp correctness/hygiene findings from the M2 build-out: `enumNames` round-trip loss (untagged ordering — mechanism verified, repro in hand), under-specified dependency floors (four named, with introduction-point evidence), `Implementation::from_build_env()` self-reporting | [3.8, 3.9](01-ecosystem-context.md) | Bug report with failing-test reproduction — **filed as [rust-sdk#903](https://github.com/modelcontextprotocol/rust-sdk/issues/903)** (2026-06-11; dossier in [#10](https://github.com/tomtom215/mcp-conformance/issues/10)); **enumNames RESOLVED — fixed by merged [rust-sdk#905](https://github.com/modelcontextprotocol/rust-sdk/pull/905) (2026-06-20), maintainer-authored per our report (a successful engagement, not *our* M4 PR); adopt on release — deferral `adopt-rmcp-enumnames-fix`**; still queued and now **drafted ready-to-file** ([docs/reports/rmcp-build-out-hygiene-draft.md](../reports/rmcp-build-out-hygiene-draft.md), 2026-06-27, owner-gated): the three rmcp-keyed floor bumps (`tokio-util` ≥0.7.9, `tokio-stream` ≥0.1.1, `tracing` ≥0.1.41 — verified against rmcp 1.7.0's published manifest) and the `from_build_env` finding (undocumented; reports rmcp's own identity via `env!` at `model.rs:1057`, wired as the `server_info`/`client_info` default). Honest caveat in the draft: `-Z minimal-versions` support is contentious upstream, so the floors are a decliney ask; `from_build_env` is the higher-confidence piece |
| 10 | rmcp streamable-HTTP client SSE-resumption gap (a POST response's SSE stream is wrapped without reconnection logic, so an in-flight request is lost on early close and `Last-Event-ID` resume never happens). **Re-decided 2026-06-27 — not filing the add-resumption fix:** the `2026-07-28` draft *removes* SSE resumability + `Last-Event-ID` from Streamable HTTP ([register 1.5d](01-ecosystem-context.md) Major #9, SEP-2575), obsoleting the proposed fix; the durable residue is rmcp's *hang* on early POST-stream close vs the re-issue the new text requires | [3.12, 1.5d](01-ecosystem-context.md) | Was "bug report with the `sse-retry` repro"; now re-scoped to the hang and **deferred to the post-spec window** — judge against the final text + draft suite scenarios (deferral `rmcp-sse-resumption-upstream-filing`, review-by 2026-09-01). `2025-11-25` mechanism re-verified first-hand 2026-06-27 at head `eb435c6`; dossier carries the reconciliation ([docs/reports/rmcp-sse-resumption-dossier.md](../reports/rmcp-sse-resumption-dossier.md)) |

Ordering follows credibility economics: small, obviously-correct contributions (2, 3) earn
the standing that large ones (1) require.

## Conduct and process norms

- Operate within MCP's governance: SEP process for anything spec-adjacent
  ([register 2.9](01-ecosystem-context.md)), maintainer priorities respected, no
  end-runs around the official suite's authority.
- Issue-first, small PRs, design notes for anything non-obvious. Review bandwidth is the
  scarcest upstream resource; arriving with a 5,000-line PR is an imposition, not a gift.
- No rebranding upstream work, no "blessed by" implications, no conformance verdicts about
  third parties published without reproducible artifacts
  ([03-conformance-strategy.md](03-conformance-strategy.md)).
- Communication channels: upstream issue trackers first; design notes in this repo; no
  announcement theater before M2 produces something demonstrable.

## Adoption plan

Adoption is the success criterion ([00-charter.md](00-charter.md)), pursued in this order:

1. **Be adoptable:** the validator runs from a single static binary with no Node dependency;
   JUnit output drops into any CI; the trace format is documented with worked examples
   (M1 DoD).
2. **Prove it on ourselves:** this repo's own CI is the first reference deployment
   (agreement check, golden corpus, tier artifacts).
3. **Offer it where the pain is:** rust-sdk's Tier-1 path ([register 2.8](01-ecosystem-context.md)),
   community Rust SDKs (`pmcp`, `rust-mcp-sdk` — [register 5.7](01-ecosystem-context.md))
   who lack any conformance story, and SEP authors needing SEP-2484 scenarios.
4. **Lower the integration cost:** a GitHub Action wrapper once (and only once) an external
   user exists to need it — shipping integrations before users is inventory, not progress.

## What we never do

- Compete with rmcp or fragment the SDK space ([ADR-0002](decisions/0002-product-scope.md)).
- Publish the bare `mcp-conformance` crate name out from under the rust-sdk's internal
  package ([ADR-0003](decisions/0003-crate-naming.md)).
- Convert goodwill into pressure: upstream owes this project nothing; unmerged offers are
  recorded and moved past, not relitigated.
