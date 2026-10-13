<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Security Model

**Status:** Active
**Last reviewed:** 2026-10-13

---

A conformance toolkit occupies a strange trust position: it parses adversarial input by
design (traces from arbitrary implementations), it spawns and talks to arbitrary processes
(SUTs), and its reference servers exist to be probed. The threat model is written for that
reality, not borrowed from a generic library template.

## Assets and trust boundaries

| Asset | Threat | Boundary |
|-------|--------|----------|
| Validator process | Malicious trace files (parser exploitation, resource exhaustion) | Traces are untrusted input. Hard caps on event count and line size (`--max-events`, `--max-line-bytes`) and on nesting depth (serde_json's 128-level recursion limit; a deeper line is a malformed trace, exit 3); typed errors, never panics; fuzzed weekly and on every PR. |
| Host machine running the everything server | The server is intentionally permissive (it exercises *every* capability) — it must never be reachable from a hostile network | Loopback bind by default; `Host`/`Origin` validation on by default; disabling it requires the self-describing `--dangerously-allow-any-host` flag, **and a server started that way says so on stderr, after its readiness line** (2026-08-20). The flag's name was the whole control, and it lives in the operator's command line — a running process left no evidence the protection was off, so a log could not be audited for it. |
| Reference host | Malicious or compromised SUT servers (hostile tool results, oversized streams, slow-loris SSE) | A run is bounded in time and effort: a hard deadline (`--deadline-secs`, default 25 s), a turn limit, an error budget, and cooperative cancellation. A stdio server's command is split and spawned without a shell, and nothing a server sends is executed. **The host sets no size limit of its own on a single response**; whatever bound rmcp's transports apply is the bound it has, and the deadline is what ends a server that streams without end. |
| CI | Supply-chain attacks via actions or dependencies | Actions pinned by SHA; `cargo deny` + `cargo audit` gates; lockfiles; trusted publishing (no long-lived tokens to steal). |
| CI's `GITHUB_TOKEN` | A job holding a write scope is a lever for anything that can influence what that job runs | Workflow default is `contents: read`; a write scope is granted per job, never per workflow, and carries an inline comment naming what it writes. |
| Trace capture (`mcp-trace-capture`) | A recording proxy reachable from a hostile network would relay anything to the upstream | Listens on loopback by default (`--listen 127.0.0.1:8080`); binding elsewhere is an explicit choice. Forwards credentials (it must be transparent) but records only the header allowlist. Termination signals sent to the stdio wrapper are relayed to the server's process group, which is killed 1 s later if still running — inside the 2 s the official SDKs' stdio clients allow between `SIGTERM` and `SIGKILL`, so their shutdown never orphans the server; only a `SIGKILL` reaching the wrapper itself sooner can. **The proxy defeats a server's `Host`-based DNS-rebinding check by default**: it sends the upstream's own name in `Host`, as a reverse proxy must for a virtually hosted server, so that check passes for every request it forwards, including one a web page has a browser send to the proxy's loopback address under a rebound name. The proxy checks no `Host` itself; `Origin` is forwarded unchanged; `--preserve-host` passes the client's `Host` through, and CI runs the official suite that way so its `dns-rebinding-protection` scenario judges the server's own check. On Windows, which has no process groups, a launcher's children (`npx` → `node`) can outlive the session ([the capture README](../../crates/mcp-trace-capture/README.md#windows)). |
| `mcp-conformance test` | It runs code: the subject (`--command` and `--client-command` are run as given — the client command by the suite, through a shell), Node, and the suite package npx fetches from the npm registry; and it listens | Installs nothing: without Node.js 18+ and npx on `PATH` it stops and says what to install. The suite is fetched only as `@modelcontextprotocol/conformance@<pin>` — an exact version, the one written in `crates/mcp-conformance/src/suite.rs` and gated by `cargo xtask conformance` and the weekly `suite-currency` — with npx run in the run's own work directory (no caller `node_modules` consulted), and the suite's `--version` is read back and compared with the pin before anything runs; another version is refused. Everything it starts listens on `127.0.0.1`, on an OS-assigned port, for the run only: the recorder's proxy (whose `Host` handling is the row above's; for a loopback server it passes `Host` through) and, for a stdio server, the bridge, which answers only requests whose `Host` is its own address and whose `Origin`, if any, is loopback, so a web page cannot drive the server through it. A stdio server's command is split at whitespace and spawned without a shell, one process per suite session, each under the recorder's stdio wrapper and stopped (process group) when its session ends. Recorded traces hold message bodies in full and are kept in the work directory. |
| Trace corpora | Secrets accidentally recorded into fixtures | Capture records headers by allowlist, so `Authorization`, cookies and other credentials are never written; the reference host captures at the message seam, where headers are unobservable. **Message bodies are recorded in full** (as parsed JSON) — a secret inside a JSON-RPC payload is recorded like any other value — so corpus review is part of PR review. A message parsing would change (an integer past 64 bits, a repeated member name) is recorded as its raw line over stdio and left out over HTTP, and counted either way. |

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
3. **Ecosystem follow-through.** [`RUSTSEC-2026-0189`](https://rustsec.org/advisories/RUSTSEC-2026-0189.html)
   now covers this CVE for `rmcp < 1.4.0`, so `cargo audit` flags vulnerable versions
   ([register 4.3](01-ecosystem-context.md)); it was filed upstream independently of this
   project ([07-ecosystem-engagement.md](07-ecosystem-engagement.md), backlog item 2) — security posture includes
   the ecosystem's tooling, not only our code.

Precision note: this advisory is DNS rebinding (CWE-346/350) only; the "CSRF" label
occasionally attached to it belongs to a different package's advisory
([register 4.4](01-ecosystem-context.md)). We do not repeat the conflation.

## CI write scopes (reviewed 2026-10-13)

No workflow grants a write scope at workflow level: every `permissions:` block
at the top of a file is `{}` or `contents: read`. Write scopes exist only on
individual jobs, and this is the complete list — six jobs across three
workflows, each scope with an inline comment in the YAML naming what it writes.

| Workflow (triggers) | Job | Write scope | For |
|---|---|---|---|
| `release.yml` (`push` tags, `workflow_dispatch`) | `package` | `id-token`, `attestations` | Sigstore OIDC + build-provenance attestation of the `.crate` files, and an SBOM attestation binding each to its CycloneDX SBOM |
| `release.yml` | `binaries` | `id-token`, `attestations` | Sigstore OIDC + build-provenance attestation of each prebuilt archive |
| `release.yml` | `github-release` | `contents` | create the release, upload `.crate` + `SHA256SUMS`, the SBOMs + `SHA256SUMS-sbom`, the binary archives + `SHA256SUMS-binaries` |
| `release.yml` | `publish` | `id-token` | crates.io Trusted Publishing (no long-lived token exists to steal) |
| `pages.yml` (`push` to the default branch, a completed `Release` run via `workflow_run`, `workflow_dispatch`) | `deploy` | `pages`, `id-token` | deploy the versioned book, built by the unprivileged `build` job; `deploy` checks out and compiles nothing |
| `scheduled.yml` (`schedule`, `workflow_dispatch`) | `claims-expire` | `issues` | open, comment on, and close the tracking issue a red claims-expiry run files ([ADR-0010 §Amendment](decisions/0010-deferral-ledger-and-scheduled-reverification.md)) |

`ci.yml` and `mutants.yml` — the only two workflows that run on
`pull_request`, and so the only two that ever execute contributor-authored
code — hold no write scope anywhere, at either level. No workflow uses
`pull_request_target`.

Two further properties bound the `issues: write` grant, the only scope held
outside `release.yml` and `pages.yml`. `scheduled.yml` triggers only on `schedule` and `workflow_dispatch`, so
no fork or pull-request content reaches the token. And the issue body that job
posts is composed from the committed deferral ledger and its own step
outcomes. Two steps in that job read the network — `spec-drift` fetches
specification text and the published pages' HTML (for their heading anchors),
and `suite-currency` fetches npm dist-tags — and neither
step's fetched text is republished into an issue; the run log carries it.

## Secrets and data hygiene

- No secrets in the repository, fixtures, or benchmarks — enforced by review plus secret
  scanning.
- The reference host treats auth material as opaque and never logs it. Trace capture keeps
  credentials out of a trace by recording headers from an allowlist, not by redaction:
  message bodies are recorded in full, with no opt-out because there is nothing to opt out
  of, and the user-facing docs say to read a trace before sharing it.
- Fuzz and property tests never use real-looking credentials as seeds.

## Vulnerability handling (our own)

Adopted from a2a-rust, and stated for reporters in [`SECURITY.md`](../../SECURITY.md):

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
