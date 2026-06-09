<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Security Model

**Status:** Active
**Last reviewed:** 2026-06-09

---

A conformance toolkit occupies a strange trust position: it parses adversarial input by
design (traces from arbitrary implementations), it spawns and talks to arbitrary processes
(SUTs), and its reference servers exist to be probed. The threat model is written for that
reality, not borrowed from a generic library template.

## Assets and trust boundaries

| Asset | Threat | Boundary |
|-------|--------|----------|
| Validator process | Malicious trace files (parser exploitation, resource exhaustion) | Traces are untrusted input. Hard caps on event count, payload size, and nesting depth; typed errors, never panics; fuzzed continuously. |
| Host machine running the everything server | The server is intentionally permissive (it exercises *every* capability) — it must never be reachable from a hostile network | Loopback bind by default; `Host`/`Origin` validation on by default; startup banner states the server is a test fixture, not a production component. |
| Reference host | Malicious or compromised SUT servers (hostile tool results, oversized streams, slow-loris SSE) | Response size and time budgets; bounded concurrency; cooperative cancellation; no shell interpretation of server-supplied strings. |
| CI | Supply-chain attacks via actions or dependencies | Actions pinned by SHA; `cargo deny` + `cargo audit` gates; lockfiles; trusted publishing (no long-lived tokens to steal). |
| Trace corpora | Secrets accidentally recorded into fixtures | Capture tooling redacts `Authorization`/cookie headers and token-shaped strings by default; corpus review is part of PR review. |

## Designing out the CVE-2026-42559 class

The defining recent vulnerability in this ecosystem is rmcp's DNS-rebinding advisory
(GHSA-89vp-x53w-74fx, CVSS 8.8: streamable-HTTP server accepted requests without validating
the `Host` header — [register 4.1–4.2](01-ecosystem-context.md)). Our posture:

1. **Default-secure construction.** Everything-server and any HTTP listener we ship validate
   `Host` and `Origin` against an allowlist defaulting to
   `localhost` / `127.0.0.1` / `::1`, returning 403 otherwise — matching the upstream fix's
   semantics. Disabling validation requires a long, ugly, documented method name; there is no
   config file flag that quietly turns it off.
2. **Conformance pressure.** The requirement registry includes the transport-security
   requirements, so *every implementation we validate* gets checked for this class — the
   toolkit propagates the fix's lesson across the ecosystem rather than just avoiding the
   bug itself.
3. **Ecosystem follow-through.** No RustSec advisory exists for this CVE, so `cargo audit`
   is silent on vulnerable rmcp versions ([register 4.3](01-ecosystem-context.md)). Filing
   it, in coordination with rmcp maintainers, is on the contribution backlog
   ([07-ecosystem-engagement.md](07-ecosystem-engagement.md)) — security posture includes
   the ecosystem's tooling, not only our code.

Precision note: this advisory is DNS rebinding (CWE-346/350) only; the "CSRF" label
occasionally attached to it belongs to a different package's advisory
([register 4.4](01-ecosystem-context.md)). We do not repeat the conflation.

## Secrets and data hygiene

- No secrets in the repository, fixtures, or benchmarks — enforced by review plus secret
  scanning.
- The reference host treats auth material as opaque and never logs it; trace capture redacts
  by default (opt-out only for synthetic-credential test environments, and the opt-out names
  itself accordingly).
- Fuzz and property tests never use real-looking credentials as seeds.

## Vulnerability handling (our own)

Adopted from a2a-rust, effective at M0 when `SECURITY.md` lands:

- Private reporting via GitHub draft security advisories (no public issues for
  vulnerabilities); acknowledgment within 3 business days.
- 90-day coordinated disclosure, negotiable with the reporter; fixes target well inside it.
- Advisories published as GHSA **and** filed with RustSec for any published crate — the rmcp
  gap is the cautionary tale.
- Reporters credited unless they request anonymity.
- P0 calibration follows the ecosystem's own definition: CVSS ≥ 7.0 or core functionality
  failure ([register 2.7](01-ecosystem-context.md)), with the SEP-1730 Tier-1 expectation
  (seven-day resolution) as our internal clock even though no tier obligations apply to us.

## What this model does not cover

Tool-poisoning detection, server reputation, permission brokering, and sandboxing of MCP
servers are real problems owned by other tools (mcp-scan/agent-scan, agentox, gateway
policy engines). We validate protocol behavior and ship secure defaults; we do not audit
intent. Scope creep into scanning would dilute both products
([ADR-0002](decisions/0002-product-scope.md)).
