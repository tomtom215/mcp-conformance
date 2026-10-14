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
| `mcp-conformance test` | It runs code: the subject (`--command` and `--client-command` are run as given — the client command by the suite, through a shell), Node, and the suite package npx fetches from the npm registry; and it listens | Installs nothing: without Node.js 18+ and npx on `PATH` it stops and says what to install. The suite is fetched only as `@modelcontextprotocol/conformance@<pin>` — an exact version, the one written in `crates/mcp-conformance-cli/src/suite.rs` and gated by `cargo xtask conformance` and the weekly `suite-currency` — with npx run in the run's own work directory (no caller `node_modules` consulted), and the suite's `--version` is read back and compared with the pin before anything runs; another version is refused. Everything it starts listens on `127.0.0.1`, on an OS-assigned port, for the run only: the recorder's proxy (whose `Host` handling is the row above's; for a loopback server it passes `Host` through) and, for a stdio server, the bridge, which answers only requests whose `Host` is its own address and whose `Origin`, if any, is loopback, so a web page cannot drive the server through it. A stdio server's command is split at whitespace and spawned without a shell, one process per suite session, each under the recorder's stdio wrapper and stopped (process group) when its session ends. Recorded traces hold message bodies in full and are kept in the work directory. |
| `mcp-conformance watch` | The recorder's exposure (it runs the stdio server, and in `http` mode it is a proxy anyone who can reach it can use), plus two of its own: an escape sequence in recorded traffic rewriting the terminal it prints findings to, and span data sent to a collector — possibly across a network — that the user did not mean to share | It *is* `mcp-trace-capture` for everything that touches the session (the same library code path, `run::stdio`/`run::http`): the stdio command is spawned without a shell, and it listens nowhere in `stdio` mode and only on `--listen` in `http` mode — `127.0.0.1:8080` by default, with the recorder's warning for any other address; the rows above for the recorder apply unchanged. It forwards bytes unchanged and never waits on judging or export: the recorder hands each line to an unbounded channel and goes on, and a slow judge is tested not to slow the relay. Findings, the tally and the report go to stderr only, every quoted string through the validator's `printable` (control characters as `\u{…}`). It sends nothing anywhere unless `--otlp` names a collector, and then only span fields — method, tool and prompt names, resource URIs, request and session IDs, JSON-RPC error codes and messages, and findings; never a payload (no opt-in attribute of the conventions is set). An `http://` collector that is not loopback is warned about at start; credentials in the `--otlp` URL are refused and the URL is never echoed with its query. Export runs off the session's path through a bounded queue (4,096 spans, dropped and counted when full), each request bounded (2 s to connect, 5 s in all), with no retries; the end of a run waits at most 10 s for it. Trace and span ids come from the OS CSPRNG (`ring`'s `SystemRandom`). |
| The HTML report (`--format html`) | A hostile recording turned into a page that runs script, loads a tracker, or exfiltrates the trace when opened — the page is made to be shared and opened by others | Everything from the trace (and from a custom registry) is escaped as text, control and bidirectional-override characters shown as `\u{…}`; only `http(s)` URLs become links. No script at all; a `<meta>` Content Security Policy (`default-src 'none'`, `style-src` the inline stylesheet's SHA-256 digest computed at render time, `base-uri` and `form-action` `'none'`) refuses every fetch and every injected style, and `no-referrer` keeps the file's location out of a followed link. Tested by a trace carrying `<script>` and `"><img onerror=…>` in a method name, an id, a header and a payload, and in the screenshot suite by recording and aborting every request a page makes (there must be none) and failing on any console error, where a CSP violation is reported. |
| The workbench (`mcp-conformance serve`) | A local web server showing recordings that may hold credentials, reachable by every page the user's browser opens: DNS rebinding, cross-site requests, a hostile recording turned into script, the token leaking | See [The workbench](#the-workbench) below: loopback bind (anything else needs `--dangerously-listen-non-loopback` and warns), `Host` and `Origin` checked on every request, a single-use launch token exchanged for an `HttpOnly`, `SameSite=Strict` cookie, changes only as same-origin JSON, a CSP with no inline script, trace text only ever a text node. It never makes an outbound request. |
| Trace corpora | Secrets accidentally recorded into fixtures | Capture records headers by allowlist, so `Authorization`, cookies and other credentials are never written — the server's `WWW-Authenticate` challenge is on the list (2026-10-13), because it requests a credential and carries none, and no request target (path or query, where a misplaced token would sit) is recorded at all; the reference host captures at the message seam, where headers are unobservable. **Message bodies are recorded in full** (as parsed JSON) — a secret inside a JSON-RPC payload is recorded like any other value — so corpus review is part of PR review. A message parsing would change (an integer past 64 bits, a repeated member name) is recorded as its raw line over stdio and left out over HTTP, and counted either way. |

## The workbench

`mcp-conformance serve` ([ADR-0021](decisions/0021-a-workbench-people-keep-open.md),
decision 4; code in `crates/mcp-conformance-cli/src/workbench/`) is the toolkit's one
long-running server meant for a browser. Its library holds recordings in full, and an
imported one (`mcpsnoop`, MCP Inspector) or a hand-made one can carry `Authorization`,
cookies and secrets in payloads. Every defense below is on the request path of every
request, a refusal included, and each has a test.

| Threat | Mitigation | Tested by |
|--------|------------|-----------|
| Another machine reaches the port | Binds `127.0.0.1:0` by default; a non-loopback `--listen` is refused unless `--dangerously-listen-non-loopback` is given, and a workbench started that way says so on stderr | `server.rs` `only_loopback_is_bound_without_the_flag`; `tests/cli/library.rs` `serve_refuses_a_non_loopback_address_without_the_flag` |
| DNS rebinding: a hostile name resolving to `127.0.0.1` lets a page read the API as same-origin | `Host` must be the bound `ip:port` (or `localhost:port` on loopback), exactly; refused with 403 before anything else is read | `guard/tests.rs` `host_must_be_the_bound_address`; `api/tests.rs` `a_rebound_host_or_a_foreign_origin_is_refused_even_with_the_cookie` |
| A page on another origin (another local port included) sends requests | `Origin`, when present, must be the workbench's own; `Sec-Fetch-Site: cross-site` is refused; no CORS header is ever sent, so a cross-origin `fetch` cannot read an answer | same tests; `every_response_is_hardened` |
| Cross-site request forgery of a change (delete, redact, import, retention) | Every non-`GET` request must carry the workbench's `Origin` and a `Content-Type: application/json` body (which no HTML form can send); the cookie is `SameSite=Strict` | `a_rebound_host_or_a_foreign_origin_is_refused_even_with_the_cookie` (a forged delete with no `Origin`, and a form-encoded one, delete nothing) |
| Any local process or page that guesses the port reads the library | A launch token (32 bytes from the OS's random source, `ring::rand::SystemRandom`) printed in the link works once: it is exchanged for a different 32-byte session secret in an `HttpOnly` cookie named for the port, and the browser is redirected to a URL without it; every other request needs the cookie (constant-time comparison) | `the_launch_link_works_once_and_only_with_its_token`, `only_the_session_cookie_authenticates`, `the_launch_link_sets_the_cookie_once_and_redirects_away_from_itself`; end to end in the screenshot suite (`the launch link works once`) and in `tests/cli/library.rs` against the real binary |
| A hostile recording runs script or loads something when viewed | Trace content is sent as JSON strings and put on the page only through `dom.js`, which creates text nodes; a test fails if any shipped script uses `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval` or `new Function`. Control characters and bidirectional overrides are shown as `\u{…}`. Every response carries `Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`, `nosniff`, `no-referrer`, `no-store`, `X-Frame-Options: DENY` and same-origin opener and resource policies; links leave the page only to `http(s)` URLs, with `noreferrer` | `assets.rs` `no_script_turns_a_string_into_markup_or_code`; `api/tests.rs` `a_hostile_recording_reaches_the_page_as_json_strings_only`; the screenshot suite's `a hostile recording renders as text and runs nothing` (every payload of `visual/fixtures/library/hostile-markup.jsonl` opened: no element made, no dialog, the escapes visible) and its record-and-abort of every request off the workbench's origin on every view |
| A shared redacted copy still holds the secret | Redaction writes a new entry, never in place, replacing the values named (headers, JSON pointers, `_meta`, literal strings) and refusing a line it cannot parse; the sidecar names each rule and a count, never a value | `redact/tests.rs` `the_redacted_copy_holds_no_secret_byte` (the four secrets of `leaky-session.jsonl` are absent from the copy and its summary), the API and CLI tests, and the browser test reading the copy's file |
| The workbench as an exfiltration path | It makes no outbound request; the page's CSP allows connections only to its own origin, and the screenshot suite fails on any request elsewhere | the screenshot suite |
| Terminal escape sequences from a recording | `library list` and `library stats` show control and bidirectional characters as `\u{…}` | `stats/tests.rs` `the_text_form_says_what_the_json_says` |

What it does not defend against: anyone who holds the session cookie or the unused launch
link (they are the user, as far as it can tell); a local user who can read the process's
command line while `--open` passes the link to the browser, or the terminal it was printed
to; another web server on `127.0.0.1` that the user's browser visits, which is sent the
workbench's cookie because cookies do not separate ports (it is `HttpOnly`, so no page can
read it, but that server could); and a library directory others can write, whose sidecars
it trusts for tags and provenance (never for a verdict: a cache is used only for the bytes
it was computed from).

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
individual jobs, and this is the complete list — nine jobs across three
workflows, each scope with an inline comment in the YAML naming what it writes.

| Workflow (triggers) | Job | Write scope | For |
|---|---|---|---|
| `release.yml` (`push` tags, `workflow_dispatch`) | `package` | `id-token`, `attestations` | Sigstore OIDC + build-provenance attestation of the `.crate` files, and an SBOM attestation binding each to its CycloneDX SBOM |
| `release.yml` | `binaries` | `id-token`, `attestations` | Sigstore OIDC + build-provenance attestation of each prebuilt archive |
| `release.yml` | `image` | `packages`, `id-token`, `attestations` | push the container image to `ghcr.io/tomtom215/mcp-conformance` (tag runs only; a rehearsal builds and runs it and pushes nothing), and Sigstore OIDC + build-provenance attestation of its digest, pushed to the registry beside it |
| `release.yml` | `github-release` | `contents` | create the release, upload `.crate` + `SHA256SUMS`, the SBOMs + `SHA256SUMS-sbom`, the binary archives + `SHA256SUMS-binaries` |
| `release.yml` | `publish` | `id-token` | crates.io Trusted Publishing (no long-lived token exists to steal) |
| `release.yml` | `publish-npm` | `id-token` | npm trusted publishing and `--provenance` for the six npm packages (tag runs only; `release` environment) |
| `release.yml` | `publish-pypi` | `id-token` | PyPI trusted publishing and PEP 740 attestations for the five wheels (tag runs only; `release` environment) |
| `pages.yml` (`push` to the default branch, a completed `Release` run via `workflow_run`, `workflow_dispatch`) | `deploy` | `pages`, `id-token` | deploy the versioned book, built by the unprivileged `build` job; `deploy` checks out and compiles nothing |
| `scheduled.yml` (`schedule`, `workflow_dispatch`) | `claims-expire` | `issues` | open, comment on, and close the tracking issue a red claims-expiry run files ([ADR-0010 §Amendment](decisions/0010-deferral-ledger-and-scheduled-reverification.md)) |

The `image` job compiles while it holds `packages: write`: the Docker build
runs every dependency's build script, as `package` and `binaries` do under
their `id-token` grants. What bounds it is what bounds those — `Cargo.lock`
with `--locked`, the base images pinned by digest, no restored cache, a tag
pushed only after `verify` (cargo-deny included) and `tests` pass — and
GitHub documents the `GITHUB_TOKEN`'s package access as the packages
associated with the workflow's own repository. `publish` remains a job that
compiles nothing, because its grant is the one that reaches crates.io.

`publish-npm` and `publish-pypi` hold the grants that reach the two other
registries, and are shaped like `publish`: tag runs only, behind the `release`
environment's reviewer, no checkout and nothing compiled. They upload the
packages the unprivileged `registry-packages` job built from the release's
binary archives (each archive checked against `SHA256SUMS-binaries` by
`cargo xtask packages`), and only after checking that file is byte-equal to
the one the GitHub release carries — so what reaches npm and PyPI is the
attested binary. Two tools run beside those grants: npm itself, installed at
an exact version (`npm@11.19.1`) because trusted publishing needs a newer npm
than the runner image may carry, and `pypa/gh-action-pypi-publish`, pinned by
commit like every other action. Neither registry holds a long-lived token for
this project.

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
