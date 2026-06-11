<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Ecosystem Engagement

**Status:** Active
**Last reviewed:** 2026-06-09

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
| 2 | RustSec advisory for CVE-2026-42559 (`cargo audit` currently silent on rmcp < 1.4.0) | [4.3](01-ecosystem-context.md) | Coordinate with rmcp maintainers, then PR to rustsec/advisory-db |
| 3 | MSRV policy for rust-sdk (none declared; measured floor **1.88** — let-chains — invisible to cargo's MSRV-aware resolver) | [3.5](01-ecosystem-context.md) | Issue with a concrete proposal: `rust-version = "1.88"` + CI job + bump policy; our ADR-0008 probes are the evidence |
| 4 | Conformance scenarios/fixtures where Rust runs expose suite gaps | [2.3, 3.6](01-ecosystem-context.md) | Small PRs to the conformance repo, SEP-tagged where applicable |
| 5 | 2026-07-28 stateless-rework readiness (test targets before the Tier-1 window — two weeks per SEP text, ~10 weeks observed this cycle) | [1.3, 1.5a, 2.5](01-ecosystem-context.md) | Roadmap M2.5: draft corpus + multi-revision validator support published early; findings filed upstream |
| 6 | Spec-vs-suite discrepancies discovered by the agreement check | [03-conformance-strategy.md](03-conformance-strategy.md) | Issues with reproducing traces attached |
| 7 | SEP-2484 traceability tooling: the registry already stores the `sep-NNNN.yaml` shape, so an emitter plus a completeness checker (every MUST/MUST NOT mapped, exclusions carrying tracking links) is a query over existing data — recurring per-SEP work the gate created with no owning tool | [2.9, 2.11](01-ecosystem-context.md) | Offer upstream as a conformance-repo utility once M1 publishes; design note first |
| 8 | Suite reliability where it scores tiers: tier-check counting bug and SDK-repo lifecycle boilerplate, both open upstream | [2.13](01-ecosystem-context.md) | Small, obviously-correct PRs to the conformance repo — credibility builders shaped like backlog items 2 and 3 |
| 9 | rmcp correctness/hygiene findings from the M2 build-out: `enumNames` round-trip loss (untagged ordering — mechanism verified, repro in hand), under-specified dependency floors (four named, with introduction-point evidence), `Implementation::from_build_env()` self-reporting | [3.8, 3.9](01-ecosystem-context.md) | Bug report with failing-test reproduction — **filed as [rust-sdk#903](https://github.com/modelcontextprotocol/rust-sdk/issues/903)** (2026-06-11; dossier in [#10](https://github.com/tomtom215/mcp-conformance/issues/10)); still queued: floors PR; one-line docs fix |

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
