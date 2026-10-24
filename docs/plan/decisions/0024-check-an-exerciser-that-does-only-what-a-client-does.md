<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0024: `check` — an Exerciser That Does Only What a Conforming Client Does

**Date:** 2026-10-24
**Status:** Proposed (written in an implementation session; the maintainer accepts or
rejects it before the command ships in a release)
**Author:** Tom F. (maintainer); drafted by Claude Code

---

## Context

The project's value is the clause-level reading of a recorded session. Getting a recording
of *your own server* is the step that loses people, and an outside-author dogfood run on
2026-10-24 (the documented commands, against Node, Python and Go servers) showed why:

1. **`mcp-trace-capture` needs a client.** The quickstart records a session a client
   already drives. An author with a server and no client has to write one first.
2. **`mcp-conformance test` is built for one subject.** It runs the pinned official suite,
   whose server scenarios call the suite's own fixture tools, prompts and resources
   (`test_simple_prompt`, `test://static-text`). Against `@modelcontextprotocol/server-everything`
   (stdio) it printed `28 scenarios, 28 checks: 18 FAILURE, 10 SUCCESS` (reproduced the same
   day), with failures such as `Prompt test_prompt_with_arguments not found`. Against a
   FastMCP server and a Go SDK server the dogfood run ended with `agreement: does not hold`:
   the suite's *client* calls `logging/setLevel`, `completion/complete` and
   `resources/subscribe` on servers that never declared those capabilities, so `LIFE-009`
   fails. In the kept FastMCP recording, the event at that finding's `seq` is a
   client-to-server `logging/setLevel`: a finding about the suite's client, charged to the
   server, because a finding does not yet say which party broke the clause. Neither result
   is a verdict about the server's own surface.
3. **`mcp-reference-host` can drive a session** (`--server-cmd`, `--sweep`, `--cancel`,
   `--subscribe`), but it is built on rmcp, is not in the release archives or the npm and
   PyPI packages, and by default calls **every** tool a server lists with synthesised
   arguments. Pointed at a real filesystem or database server that is not safe.

What is wanted is one command that takes a server and returns the clause report, without
writing a client and without risking the server.

The charter's non-goals bear on this: the project is not an SDK, not a gateway
([ADR-0019](0019-a-recording-tap-is-not-a-gateway.md)) and not a security scanner
([ADR-0002](0002-product-scope.md)). An exerciser is a *test client*, and the line to hold
is that it behaves as a conforming client would.

## Decision

1. **Add `mcp-conformance check (--url <URL> | --command <CMD>)`.** It records through the
   recorder the way `test` does (the in-process proxy for a URL; the stdio wrapper, reached
   through the existing loopback bridge, for a command), drives the exerciser below, judges
   the recording with the unchanged engine, and prints the report. Formats: human, JSON,
   JUnit, SARIF. Exit status: `0` judged and nothing failed, `1` findings, `2` could not be
   run or judged — the same table as `validate` and `test`.
2. **The exerciser sends only what a conforming client sends.** Every request is
   spec-defined, well-formed, and addressed to something the server itself listed. It sends
   no malformed bytes, no unlisted names, no oversized or repeated input, and nothing aimed
   at a vulnerability. Provoking a server's *refusals* is a different capability with a
   different risk and is not decided here.
3. **Reads by default; writes only on request.** `tools/call` is made only for a tool the
   server annotates `readOnlyHint: true` (and not `destructiveHint: true`). `--call <name>`
   (repeatable) and `--call-all` are explicit. Every listed tool that is not called appears
   in the report with the reason (`not read-only`, `no arguments could be built from its
   schema`, `over the budget`). Arguments are built only from the tool's own `inputSchema`
   (required members; `enum`, `const`, `default`, `minimum`/`maximum`, `minLength`,
   `format`); when none can be built, the tool is skipped, never guessed at.
4. **Bounded.** Defaults: at most 100 requests and 25 tool calls per session, 30 s per
   request, 5 minutes in all, 2 MiB read per response, pagination followed for at most 20
   pages, no retries, no concurrency. Each bound is a flag and each is reported when it
   stopped the run.
5. **The exerciser declares no client capabilities** (no sampling, roots or elicitation),
   so a conforming server does not ask for them. If a server returns an `input_required`
   result or a server-initiated request anyway, the exerciser records it, answers the
   protocol-required minimum (a decline), and reports that the step needed client input.
6. **It links no MCP SDK.** The wire is JSON-RPC over the recorder's transports, written
   by hand, so the recording describes bytes and not an SDK's reading of them
   ([02-architecture.md](../02-architecture.md)). `mcp-reference-host` stays the rmcp-based
   client under test; the duplication of "enumerate and read" between the two is accepted.
7. **Revision.** `--spec-version auto` (the default) behaves as current clients do: try the
   stateless `server/discover` of `2026-07-28`, fall back to the `initialize` handshake of
   `2025-11-25` when the server does not speak it, and record both attempts. An explicit
   revision speaks that one only.
8. **The report is not a certificate.** It states what was exercised (counts of tools,
   resources and prompts listed, read, called, skipped) and leaves every clause the session
   did not reach as *not observed*, as every report does. Findings are shown for the
   server first, attributed by party (the report model's `party`), so a finding caused by
   the exerciser's own choices is visible as such.
9. **It does not replace `test`.** `test` remains the way to run the official suite; its
   output says, when a subject is not the suite's fixture server, that fixture scenarios
   are expected to fail there and points to `check`.
10. **Verified in CI.** `cargo xtask conformance` gains a leg: `check` against the everything
    server over HTTP and stdio at both revisions must judge clean, and its argument
    synthesis is property-tested against the JSON Schema engine already in the tree (every
    built argument validates against the schema it was built from).

## Consequences

### Positive

- "My server" to "which clauses it met, broke, or never reached" is one command, with no
  client to write and no Node dependency for the exerciser itself.
- The same recorder and engine judge it, so a `check` finding is as reproducible as any
  other: the trace is kept and can be re-judged, diffed, or opened in the workbench.
- The read-only default makes it safe to point at a real server in CI or on a laptop.

### Negative

- A second client implementation to maintain beside the reference host. A bug in the
  exerciser can look like a server defect; the party attribution, the kept trace and the
  CI leg against a known-conforming server are the mitigation, not a cure.
- A read-only default under-exercises servers whose tools are not annotated: most are not
  today. The report says so plainly, and `--call` is the remedy, which moves responsibility
  for side effects to the person who typed it.
- The CLI crate gains an HTTP client dependency (hyper, already in the tree through the
  recorder) and a few thousand lines.

## Alternatives Considered

- **Ship `mcp-reference-host` in the release archives and have `check` spawn it.**
  Rejected: it is rmcp-based, so the recording would describe rmcp's behaviour; it calls
  every tool; and it adds a second binary to every package. Its generic plan and `--sweep`
  are the model for what the exerciser does, not a dependency.
- **Presets of the official suite's generic scenarios under `test`.** Kept as a small
  addition (item 9), not the answer: the suite does not reach prompts, resources or tools
  the server chose to expose, and its verdicts are not clause-level.
- **Document "write a client" and leave it there.** This is the status quo that sent the
  dogfood author to hand-written scripts. Rejected.
- **Wrap the MCP Inspector's CLI.** Rejected: a Node dependency we do not control, and no
  guarantee about the bytes it sends.
