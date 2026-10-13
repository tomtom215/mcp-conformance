<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0021: A Workbench People Keep Open — `mcp-conformance`

**Date:** 2026-10-13
**Status:** Accepted (adds a sixth published crate; keeps every non-goal of
[ADR-0002](0002-product-scope.md) and [ADR-0019](0019-a-recording-tap-is-not-a-gateway.md))
**Author:** Tom F.

---

## Context

The toolkit answers one question well: which sentence of the specification a recorded
session broke, with every clause it could not see accounted for. Few people ask that
question every day, and three things keep the answer from them:

1. **Friction.** A user installs Rust crates, records with one tool, then judges with
   another. MCP developers mostly work in TypeScript and Python, where `npx` and `pip` are
   the norm; `mcpsnoop`, the nearest neighbour, installs from npm.
2. **Nothing to keep open.** The verdict arrives after the session. `mcpsnoop` shows
   traffic live, exports OpenTelemetry spans and fails CI — it is a daily tool. Ours is a
   gate.
3. **What a recording cannot show.** A recording witnesses only what the session did, so
   most clauses read `not-observed`. The official suite exercises an implementation; alone,
   it gives the clearer yes or no.

The official suite remains the authority (charter); this is not a decision to compete
with it. It is a decision to make the clause-level reading the thing a developer reaches
for while building, debugging and reviewing — and to take the suite's exercising along
rather than rebuild it.

## Decision

Ship `mcp-conformance`, one binary over the existing libraries, and three distribution
paths for it. Each phase closes against its own definition of done
([06-roadmap.md](../06-roadmap.md), section "The workbench").

1. **`mcp-conformance test`** runs the pinned official suite against a server (`--url`,
   or `--command` under the stdio wrapper) or a client (`--client-command`) *through*
   `mcp-trace-capture`, and reports both verdicts: the suite's checks and the clause
   report of the same traffic. The suite is run, never reimplemented; Node is a stated
   requirement of this one subcommand. This is the CI job `cargo xtask conformance` has
   run for months, made a product.
2. **A self-contained HTML report** (`--format html` on the validator and every
   `mcp-conformance` command): one file, no network, inline CSS and JavaScript; the message
   timeline with each finding at its event, the quoted clause beside it, and the excluded
   and not-observed accounting. It is the artifact attached to a bug report against an SDK.
3. **`mcp-conformance watch`** wraps capture (either mode) and judges as the session runs:
   each completed exchange is re-judged and new findings print with their clause ID and
   quote. Read-only, like the tap: it forwards bytes unchanged and never blocks, filters or
   rewrites (ADR-0019's line). Optional `--otlp <endpoint>` exports one span per exchange
   under the OpenTelemetry MCP semantic conventions (`mcp.method.name`, `mcp.session.id`,
   `jsonrpc.request.id`; span name `{mcp.method.name} {target}` — Development stability in
   the GenAI conventions repository, read 2026-10-13), with each finding as a span event
   carrying the requirement ID and spec URL. OTLP/HTTP with JSON encoding, built on the
   HTTP client already in the tree, not the `opentelemetry` crates.
4. **`mcp-conformance serve`**, a local workbench in the browser over a library of
   recordings:
   - *Library and search.* Recordings in one directory, each JSONL trace with a sidecar of
     tags, provenance and a verdict cache keyed by the trace's SHA-256 and the tool
     version. An in-memory index answers queries over methods, tool and resource names,
     session ids, error codes, requirement IDs and finding text
     (`method:tools/call req:BASE-003 timeout`).
   - *Debugging.* A session timeline (requests paired with responses, notifications,
     HTTP exchanges, latency), message inspector, finding-to-event navigation, two-session
     diff (what changed between SDK versions), and the live view of a running `watch`
     (the workbench tails the trace file being written; no second protocol).
   - *Dashboards.* Verdicts over time, clause coverage across the library (which clauses
     any recording has ever exercised), findings by area and by server — 2D SVG drawn
     from the same data the CLI reports, so every number has a command that reproduces it.
   - *Administration of the library.* Import, tag, retention, delete, export, and
     **redaction**: recordings carry bearer tokens, cookies and payloads, so a redacted
     copy (headers by name, `_meta` and argument values by JSON pointer) is one action,
     and nothing is ever sent anywhere by the workbench.
   - *Importers.* `mcpsnoop` captures and MCP Inspector exports become traces, with what
     the source did not record stated, never invented.
5. **Distribution.** npm (`npx mcp-conformance`) as a main package selecting a
   per-platform optional-dependency package that carries the attested release binary (no
   install-time download), and PyPI (`pip install mcp-conformance`) as per-platform wheels
   carrying the same binary; plus the container image. Built and tested in CI on every
   change; published from the release job once the owner has configured the registries.

### The crate's name (open, owner's decision)

[ADR-0003](0003-crate-naming.md) §2 keeps the bare name `mcp-conformance` off
crates.io: the official Rust SDK names its own internal, unpublished conformance
package that, and publishing it from under them "would be hostile-adjacent". This ADR,
as first written on 2026-10-13, chose the name without noticing that decision; the
correction is recorded here rather than by editing the decision above. Until the owner
decides, the crate is `publish = false` and the binary is installed from source. The
options are a superseding ADR after asking upstream, or a different published crate
name with `mcp-conformance` kept as the binary's name. The npm and PyPI names are not
covered by ADR-0003.

### Running the suite against a stdio server

The suite tests servers only over HTTP. `test --command` therefore runs a loopback
bridge, inside `test` only, that starts one server process per session under the
capture's stdio wrapper and relays the suite's HTTP requests to it, so the recording is
the stdio wire. It changes one thing in the bytes (CR and LF in a POST body become
spaces, since a stdio message is one line), it skips the suite's three HTTP-transport
scenarios and says so in the report, and it is reachable only from loopback with `Host`
and `Origin` checked. It is a test harness, not a proxy offered to anyone: ADR-0019's
"not a gateway" line holds because nothing outside `test` can route through it.

### The front end

Plain HTML, CSS and ES-module JavaScript, embedded in the binary at compile time; charts
are hand-written SVG. No bundler, no framework, no `node_modules` in the build of a Rust
crate, no committed build output.

### Security

The workbench is a local web server showing data that may contain credentials. It binds
`127.0.0.1` by default and warns on any other address; every request must carry a random
per-launch token (printed in the URL it opens) and pass `Host` and `Origin` checks — DNS
rebinding and cross-site requests from other local pages are the threats. Responses carry
a strict Content Security Policy (no inline script in the workbench; the HTML report,
being one file, uses a hash-pinned inline script). Trace content is rendered as text,
never as HTML. Additions to [05-security-model.md](../05-security-model.md) land with the
code.

### Verification of what people see

Every view and the HTML report are rendered by a headless Chromium (Playwright, pinned)
over committed fixture libraries and compared with committed screenshots, in CI, with the
font embedded so rendering does not depend on the runner's system fonts. A visual change is
a reviewed baseline update, like a golden report. Interaction (search, navigation, diff,
redaction) is tested end to end in the same harness.

## Alternatives considered

- **Extend the existing binaries only** (`--watch` on capture, `--html` on the validator,
  no new crate). Rejected for the workbench: a web server and a trace library do not
  belong in a recorder or a pure judge, and `cargo install mcp-trace-validator` should
  stay small. The HTML report is still added to the validator, because it is a report
  format.
- **A JavaScript framework and bundler for the UI** (React, or three.js for 3D views).
  Rejected: a Node build inside a Rust crate's build, a supply chain the project does not
  otherwise carry, and committed bundles that are build output. A protocol session is a
  timeline and a set of findings; nothing in it is three-dimensional. Revisit if a view
  needs it.
- **The `opentelemetry` crates for export.** Rejected for now: OTLP/HTTP JSON is a small,
  stable encoding, and the crates bring a large, fast-moving dependency tree to emit a
  handful of span fields. Revisit if users need gRPC or the SDK's batching semantics.
- **OTLP ingestion** (judge sessions from spans an SDK emitted). Not now: the MCP
  conventions do not carry message payloads, so spans cannot reconstruct what the
  validator judges. Recorded as a non-goal of this ADR, not of the project.
- **A hosted dashboard.** Out of scope by the charter ("Not a hosted service").

## Consequences

### Positive

- The suite's exercising and our clause-level reading arrive from one command; the
  `not-observed` gap of a lone recording closes to what the suite exercises.
- A daily tool: live findings while developing, a shareable report for bug reports, a
  library to search when something breaks.
- Installation where MCP developers already are.

### Negative

- A sixth published crate and two new package ecosystems to release into, each with its
  own supply chain, versioning and trusted-publishing setup (owner actions).
- A front end to maintain, with screenshot baselines that change with every visual edit.
- A local web server is new attack surface; the mitigations above are mandatory, and a
  defect in them is a security issue under SECURITY.md.
- `test` depends on Node and the suite's CLI staying stable; the pinned versions and the
  existing suite-currency gate bound that.

### Neutral

- The validator stays a pure function; everything here is built around it, not in it.
