<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Ecosystem Context — Verified Facts Register

**Status:** Active
**Last reviewed:** 2026-06-09

---

This register is the **only** place in the plan where volatile external facts (versions,
download counts, tier tables, dates) are recorded. Every row carries the date it was last
verified against the listed source. Other documents link here instead of repeating numbers.

**Maintenance rules**

1. A row older than 90 days must be re-verified before it is cited in anything external
   (a README claim, an upstream issue, a report).
2. Re-verification updates the row in place — value, date, and source. Git history preserves
   the old value; the document does not.
3. A refuted row is corrected immediately and anything derived from it is re-examined; the
   derivation chain is what the `Used by` column tracks.
4. Facts with no bearing on engineering decisions do not belong here.

Statuses: **Verified** (primary source fetched on the stated date) ·
**Partial** (true with stated nuance) · **Unverified** (recorded for follow-up; must not be
cited).

## 1. Protocol and governance

| # | Fact | Status | Verified | Source | Used by |
|---|------|--------|----------|--------|---------|
| 1.1 | Current MCP protocol revision is `2025-11-25` | Verified | 2026-06-09 | [Versioning](https://modelcontextprotocol.io/specification/versioning) | Charter, Architecture |
| 1.2 | A release candidate dated `2026-07-28` exists; final ships July 28, 2026 | Verified | 2026-06-09 | [RC announcement](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/) (2026-05-21) | Architecture, Roadmap, Risks |
| 1.3 | The 2026-07-28 RC removes the `initialize`/`initialized` handshake and the `Mcp-Session-Id` header ("stateless rework") | Verified | 2026-06-09 | RC announcement: "The `initialize`/`initialized` handshake is removed." / "The `Mcp-Session-Id` header … also removed." | Architecture §state machines |
| 1.4 | Feature lifecycle policy: deprecated features remain ≥ 12 months before removal, with a 90-day expedited-removal exception | Verified | 2026-06-09 | Versioning page; RC announcement | Standards §deprecation |
| 1.5 | The RC adds an Extensions framework: reverse-DNS IDs negotiated via an `extensions` capability map | Verified | 2026-06-09 | RC announcement | Conformance strategy |
| 1.6 | MCP was donated to the Agentic AI Foundation (Linux Foundation directed fund) on 2025-12-09; governance model unchanged | Verified | 2026-06-09 | [Anthropic announcement](https://www.anthropic.com/news/donating-the-model-context-protocol-and-establishing-of-the-agentic-ai-foundation); [LF press release](https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation); [MCP blog](https://blog.modelcontextprotocol.io/posts/2025-12-09-mcp-joins-agentic-ai-foundation/) | Engagement |
| 1.7 | Scale: "97M+ monthly SDK downloads across Python and TypeScript"; "more than 10,000 active public MCP servers" (Anthropic) / "10,000 published" (LF) | Verified | 2026-06-09 | Same as 1.6 | Charter (context) |
| 1.8 | Lead Maintainers: David Soria Parra ("Lead Core Maintainer") and Den Delimarsky; affiliations are no longer published on the governance page | Verified | 2026-06-09 | [Governance](https://modelcontextprotocol.io/community/governance); [maintainer update](https://blog.modelcontextprotocol.io/posts/2026-04-08-maintainer-update/) | Engagement |

## 2. Conformance and tiering

| # | Fact | Status | Verified | Source | Used by |
|---|------|--------|----------|--------|---------|
| 2.1 | `modelcontextprotocol/conformance` is the official suite: TypeScript, npm `@modelcontextprotocol/conformance`, tests **both** clients and servers | Verified | 2026-06-09 | [Repo README](https://github.com/modelcontextprotocol/conformance) | Conformance strategy |
| 2.2 | Server-under-test wiring: `npx @modelcontextprotocol/conformance server --url http://localhost:3000/mcp`; client-under-test: `… client --command "<cmd>" --scenario <name>` | Verified | 2026-06-09 | Repo README | Architecture §xtask, Roadmap M2/M3 |
| 2.3 | Scenario suites: `all`, `core`, `extensions`, `backcompat`, `auth`, `metadata`, `draft`, `sep-835`; a `tier-check` subcommand evaluates a repo against SEP-1730 | Verified | 2026-06-09 | Repo README | Conformance strategy |
| 2.4 | npm package: latest stable **0.1.16** (2026-03-30); **0.2.0-alpha** line active (alpha.2, 2026-06-03); ~20,985 downloads/month | Verified | 2026-06-09 | [npm registry](https://registry.npmjs.org/@modelcontextprotocol%2Fconformance) | Standards §pinning, Risks |
| 2.5 | SEP-1730 (SDK Tiering) is **Final** (created 2025-10-29; authors Inna Harper, Felix Weinberger). Tier 1: "All conformance tests pass" (100%), new features inside the two-week RC→release window, triage within two business days, security/critical bugs resolved within seven days, stable release & versioning documented. Tier 2: 80% pass, features within six months, ≥ 1 stable release, published Tier-1 roadmap | Verified | 2026-06-09 | [SEP-1730](https://modelcontextprotocol.io/seps/1730-sdks-tiering-system); [sdk-tiers](https://modelcontextprotocol.io/community/sdk-tiers) | Charter, Conformance strategy, Roadmap |
| 2.6 | SEP-1730 appendix asks SDK maintainers to "implement everything server based on a spec … check it into each SDKs repo" | Verified | 2026-06-09 | SEP-1730 appendix | Charter, Roadmap M2 |
| 2.7 | "Critical Bug" = P0: security vulnerabilities with CVSS ≥ 7.0 or core functionality failures | Verified | 2026-06-09 | sdk-tiers page | Security model |
| 2.8 | Published tier table: **Tier 1** TypeScript, Python, C#, Go · **Tier 2** Java, **Rust** · **Tier 3** Swift, Ruby, PHP · Kotlin TBD (official tiering published 2026-02-23) | Verified | 2026-06-09 | [SDK docs](https://modelcontextprotocol.io/docs/sdk) | Charter, Engagement |
| 2.9 | SEP-2484 (merged 2026-05-17, author pcarleton): a Standards-Track SEP changing observable behavior cannot reach Final without a merged conformance scenario tagged with the SEP number **and** a traceability file (`sep-NNNN.yaml`) mapping each MUST/MUST NOT to a check or a documented exclusion | Verified | 2026-06-09 | [PR #2484](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2484) | Conformance strategy (traceability format) |
| 2.10 | The TypeScript everything server (`modelcontextprotocol/servers`, `src/everything`) exercises tools, prompts, resources (links/references/subscriptions), sampling (sync+async), elicitation (incl. URL mode), roots, logging toggles, progress, structured output | Verified | 2026-06-09 | [src/everything](https://github.com/modelcontextprotocol/servers/tree/main/src/everything) | Architecture §everything-server scope |

## 3. Official Rust SDK (rmcp) state

| # | Fact | Status | Verified | Source | Used by |
|---|------|--------|----------|--------|---------|
| 3.1 | `rmcp` latest release 1.7.0 (2026-05-13); 12,051,460 all-time downloads; repo ~3.5k stars, 38 open issues | Verified | 2026-06-09 | [crates.io API](https://crates.io/api/v1/crates/rmcp); [repo](https://github.com/modelcontextprotocol/rust-sdk) | Charter |
| 3.2 | License: Apache-2.0 (docs CC-BY-4.0; legacy MIT contributions noted during relicensing) | Verified | 2026-06-09 | Workspace `Cargo.toml`, LICENSE | ADR-0003, Standards |
| 3.3 | rust-sdk **already contains** a `conformance/` workspace package (internally named `mcp-conformance`, `publish = false`) with `conformance-server` and `conformance-client` binaries wired to the official suite | Verified | 2026-06-09 | [conformance/Cargo.toml](https://github.com/modelcontextprotocol/rust-sdk/tree/main/conformance) | Charter, ADR-0003 (name collision), Engagement |
| 3.4 | rust-sdk has **no everything server** (examples cover counter, memory, elicitation, prompt, task, progress, auth; `everything_stdio.rs` is a *client* example) | Verified | 2026-06-09 | Repo `examples/` tree; issue #769 | Charter, Roadmap M2 |
| 3.5 | rust-sdk declares **no MSRV** — no `rust-version` in any workspace manifest, none in README | Verified | 2026-06-09 | Workspace `Cargo.toml` | Engagement (contribution backlog) |
| 3.6 | Tier-2 process issues #690–#693 (labeling/triage, stable release, ROADMAP.md, documentation) are closed; #684 "Conformance Testing" remains open | Verified | 2026-06-09 | [Issue #684](https://github.com/modelcontextprotocol/rust-sdk/issues/684) | Engagement |
| 3.7 | Most active maintainers: 4t145 and jokemanfire; CODEOWNERS is the `@modelcontextprotocol/rust-sdk` team | Verified | 2026-06-09 | Repo CODEOWNERS, commit history | Engagement |

## 4. Security

| # | Fact | Status | Verified | Source | Used by |
|---|------|--------|----------|--------|---------|
| 4.1 | CVE-2026-42559 / GHSA-89vp-x53w-74fx: rmcp's streamable-HTTP server transport did not validate the `Host` header → DNS rebinding. Severity High, CVSS 8.8, CWE-346/CWE-350. Affected < 1.4.0, patched 1.4.0. Published 2026-05-06, credited to JLLeitschuh | Verified | 2026-06-09 | [GHSA-89vp-x53w-74fx](https://github.com/modelcontextprotocol/rust-sdk/security/advisories/GHSA-89vp-x53w-74fx) | Security model |
| 4.2 | Fix shape: `validate_dns_rebinding_headers()` on all requests (403 on miss); `StreamableHttpServerConfig::allowed_hosts` defaulting to `["localhost", "127.0.0.1", "::1"]`; `with_allowed_hosts(…)` builder | Verified | 2026-06-09 | Advisory + rmcp release notes | Security model, Architecture |
| 4.3 | **No RustSec advisory exists** for CVE-2026-42559 (`rustsec.org/packages/rmcp.html` → 404): `cargo audit` users get no warning for rmcp < 1.4.0 | Verified | 2026-06-09 | [rustsec.org](https://rustsec.org/) | Engagement (contribution backlog) |
| 4.4 | The "CSRF" label sometimes attached to this CVE belongs to a different advisory (GHSA-fvh2-gm75-j4j7, npm `dynoxide`); rmcp's advisory is DNS rebinding only | Verified | 2026-06-09 | GitHub advisory DB | Security model (precision) |

## 5. Prior art and adjacent tooling

No existing tool — in any language — validates recorded MCP traces against the spec's
normative requirements, and no Rust everything server exists (5.1–5.4 are the closest
neighbors). Honest accounting:

| # | Tool | What it is | Scale (2026-06-09) | Overlap with us |
|---|------|------------|--------------------|-----------------|
| 5.1 | `tooltest` / `tooltest-core` | "CLI conformance testing for MCP servers" | 162 / 253 downloads | Live server probing; no trace validation, no requirement registry |
| 5.2 | `mcp-tester` | MCP server testing tool (lib + CLI) | 510 downloads | Same category as 5.1 |
| 5.3 | `mcp-wallfacer` / `wallfacer-core` | Runtime fuzzing and invariant testing for MCP servers | 189 / 198 downloads | Fuzzing, not conformance |
| 5.4 | `agentox` (AgentOx) | MCP security and conformance auditor | 88–153 downloads; 6 stars | Security-audit flavored |
| 5.5 | `mcp-probe` (markndg), `mcptest` (soapbucket) | Contract/compliance test runners, both created 2026-05-13 | 0–2 stars | Early, unestablished |
| 5.6 | `mcplint` | Testing/fuzzing/security scanning | small | Lint flavored |
| 5.7 | Alternative Rust SDKs: `rust-mcp-sdk` (153k downloads), `pmcp` (66.7k, "full TypeScript SDK compatibility"), `turbomcp-protocol`, `mcp-protocol-sdk` | SDKs, not conformance tools | various | Potential validator *users*, not competitors |
| 5.8 | `mcp-host` crate | "Production-grade MCP host" (seuros/mcphost-rs), active, 33 releases | 1,430 downloads | Name collision only — forced our host crate's name (ADR-0003) |
| 5.9 | `mcp-spec` crate | Name reserved 2025-02-27 by MCP maintainers (dsp-ant, baxen) for rust-sdk | 0.1.0 placeholder | Signals official intent around spec-types naming; we stay clear |

Crate-name availability snapshot (full table and decision in
[ADR-0003](decisions/0003-crate-naming.md)): `mcp-conformance`, `mcp-conformance-core`,
`mcp-trace-validator`, `mcp-everything-server`, `mcp-reference-host`, `mcp-tck`, `mcp-test`,
`mcp-validator` all returned 404 (available) from the crates.io API on 2026-06-09;
`mcp-host` and `praxis` are taken. Names in this space move fast — `mcp-host` went from
unregistered to 33 releases in five months.

## 6. Adjacent product context

Facts that motivate the non-goals in the [charter](00-charter.md) and
[ADR-0002](decisions/0002-product-scope.md):

| # | Fact | Status | Verified | Source |
|---|------|--------|----------|--------|
| 6.1 | Official Claude API client SDKs: Python, TypeScript, C#, Go, Java, PHP, Ruby — no Rust | Verified | 2026-06-09 | [platform.claude.com client SDKs](https://platform.claude.com/docs/en/api/client-sdks) |
| 6.2 | Anthropic acquired Stainless (announced 2026-05-18), which "powered the generation of every official Anthropic SDK"; hosted Stainless products including the SDK generator are winding down | Verified | 2026-06-09 | [Anthropic](https://www.anthropic.com/news/anthropic-acquires-stainless); [Stainless blog](https://www.stainless.com/blog/stainless-is-joining-anthropic/); [TechCrunch](https://techcrunch.com/2026/05/18/anthropic-has-acquired-the-dev-tools-startup-used-by-openai-google-and-cloudflare/) |
| 6.3 | Claude Agent SDK ships in Python and TypeScript only; existing community Rust "ports" wrap the `claude` CLI as a subprocess | Verified | 2026-06-09 | [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview); crates' own docs |
| 6.4 | agentgateway: Rust agentic proxy, Linux Foundation project (Solo.io contribution, 2025-08-25), hosted by the Agentic AI Foundation; 3,182 stars | Verified | 2026-06-09 | [Repo](https://github.com/agentgateway/agentgateway); [LF press](https://www.prnewswire.com/news-releases/linux-foundation-welcomes-agentgateway-project-to-accelerate-ai-agent-adoption-while-maintaining-security-observability-and-governance-302534106.html) |

Implication, in one line per non-goal: a Rust Messages-API client can be obsoleted by
Anthropic at will (6.1, 6.2); an Agent SDK port is a subprocess wrapper around a vendor
product (6.3); the Rust proxy/gateway space is owned by a Linux Foundation project with three
orders of magnitude more adoption (6.4). Conformance tooling has the opposite profile: every
spec revision *increases* its value, and 2.5/2.9 make it structurally load-bearing.
