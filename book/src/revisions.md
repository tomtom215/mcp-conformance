<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Two revisions at once

MCP is a *dated* specification. `2025-11-25` is what implementations ship
against today; `2026-07-28` removes the `initialize` handshake outright
([SEP-2575](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2575)),
makes every request carry its own `_meta` envelope, and adds caching hints,
multi-round-trip requests, subscriptions, and a discovery probe. That is not a
version bump anyone should take on faith.

So this toolkit judges **both**, and can judge one trace against both in a
single pass. A migration stops being an inventory of changes somebody read in a
changelog and becomes a measurement.

## What "both" means here

| | `2025-11-25` | `2026-07-28` |
|---|---:|---:|
| Registry entries | 142 | 272 |
| Judged by a named check | 54 | 125 |
| Carrying a documented exclusion | 88 | 147 |
| Shipped by default | yes | behind the `draft-2026-07-28` feature |

The two registries are extracted **per revision** rather than sharing entries:
each clause is quoted from its own revision's published text, and a clause
restated with different words gets its own ID. `2025-11-25`'s BASE-003 forbids
reusing a request ID *within a session*; `2026-07-28`'s BASE-045 forbids
reusing one *that is still in flight*, so reuse after a response is now legal.
Those are two clauses, not one clause with a footnote — and pointing the new
one at the old one's check would have reported conforming traces as violations.

The cost of that method is that no clause is in force at both revisions, so a
side-by-side report never shows one clause passing at one revision and failing
at the other. What it shows instead is *presence*, which is what a migration
actually consists of.

## A trace judged against both

This is a conforming `2025-11-25` handshake — the client offers a version, the
server answers, the client confirms:

```jsonl
{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"corpus-client","version":"0.1.0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"corpus-server","version":"0.1.0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
```

Judged against both revisions at once:

```text
$ mcp-trace-validator validate --revision 2025-11-25 --revision 2026-07-28 handshake.jsonl
```

Four rows out of the report say the whole thing:

```text
  LIFE-001   (MUST)  2025-11-25=pass  2026-07-28=absent  *differs
  DISC-001   (MUST)  2025-11-25=absent  2026-07-28=not-observed  *differs
  BASE-030   (MUST)  2025-11-25=absent  2026-07-28=fail  *differs
  BASE-045   (MUST NOT)  2025-11-25=absent  2026-07-28=pass  *differs
```

- **`LIFE-001`** — "the client MUST initiate the lifecycle" — is a clause the
  newer revision **removes**. It reads `pass` then `absent`.
- **`DISC-001`** — the `server/discover` probe that replaces the handshake — is
  a clause the newer revision **adds**, and this session carried nothing it
  binds to, so it reads `not-observed` rather than counting as a pass.
- **`BASE-030`** — every request must carry an
  `io.modelcontextprotocol/clientCapabilities` `_meta` member — is a clause the
  newer revision adds that this session **breaks**. It reads `fail`, and it is
  the migration work this trace has actually discovered.
- **`BASE-045`** — the narrowed request-ID rule — reads `pass`, because reusing
  an ID after its response is exactly what the new text permits.

And the verdict splits:

```text
per revision:
  2025-11-25: 17 pass, 0 fail, 0 warn, 88 excluded, 0 unsupported, 14 not applicable, 23 not observed — verdict pass
  2026-07-28: 14 pass, 8 fail, 0 warn, 147 excluded, 0 unsupported, 0 not applicable, 103 not observed — verdict fail
overall verdict: fail
```

A conforming session today, and eight concrete failures tomorrow. Both summary
lines account for **every** clause in their revision — the counts add up to 142
and 272 — because a line that quietly omits an outcome is a line that overstates
what was measured.

> The example above is executed by a test (`book_examples.rs`) against the real
> validator on every `cargo test`, so this page cannot drift from what the tool
> actually prints.

## Judged against the wrong revision

Naming a revision is a choice, and the default is `2025-11-25`. Point the
validator at a `2026-07-28` recording without saying so and every clause the two
revisions disagree about becomes a finding: the session opens with `tools/list`
rather than `initialize`, so `LIFE-001` fails; it reuses request ids after their
responses, so `BASE-003` fails. Both are correct answers to the question that was
asked, and neither is a defect in the implementation.

A trace is not silent about which revision it belongs to — the handshake states
it, a stateless session states it in every request's `_meta`, and the HTTP
transport states it in a header. This session says `2026-07-28` twice over:

```jsonl
{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"accept":"application/json, text/event-stream","mcp-protocol-version":"2026-07-28"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}}
{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"tools":[],"resultType":"complete"}}}
```

so validating it without naming a revision says so, above the rows and again
under the verdict:

```text
  NOTE  this session declares protocol revision 2026-07-28, not 2025-11-25.
        Every outcome here judges it against rules it was not playing by;
        re-run with `--revision 2026-07-28` to judge it against its own.
```

It is deliberately quiet. A session that proposed one revision and negotiated
another has touched both, so judging it against either draws no note; a version
a server *refused* states nothing, because the session never ran under it; and a
recording that declares no revision at all gets no note, since there is nothing
to disagree with.

## The four ways a clause is not a pass

A side-by-side report is where these stop being pedantry, because all four
appear at once and they mean different things:

| Reads | Means |
|---|---|
| `absent` | The clause does not exist at that revision. |
| `excluded` | It exists, but no recorded trace can judge it — the reason is stated per entry. |
| `not-applicable` | It exists and is judgeable, but it is gated on a capability this session never negotiated. |
| `not-observed` | It exists, is judgeable, is not gated — and the session simply carried nothing it binds to. |

None of them is a pass, and the report never quietly promotes one. That rule is
the whole reason the numbers here are smaller than a conformance tool's numbers
usually are.

## What is measured at `2026-07-28` today

- The reference server serves the stateless surface with
  `--protocol-version 2026-07-28`, and the official suite's `2026-07-28`
  scenarios score **41 passing / 0 failing** against it.
- Five committed captures — a conforming session over each transport, a *probe*
  session of deliberately malformed requests, and the official runner's two —
  evidence **114 of the 125 judgeable clauses** between them. What no capture
  reaches is named, one clause at a time, in [the corpus chapter](corpus.md).
- `cargo xtask draft-readiness` re-runs that measurement and **ratchets** it
  against a committed baseline: any change in either direction fails the build,
  so progress toward the next revision is recorded in the commit that earned it
  rather than estimated.

That ratchet has already paid for itself in the direction nobody plans for.
Through suite `0.2.0-alpha.9` the runner scored the `2025-11-25` server and the
`2026-07-28` server identically — its scenarios exercise features, not
revisions, so it could not tell them apart. This project's registry could, and
flagged the `2025-11-25` server for returning cacheable results without the
`ttlMs` hint the new revision requires (CACH-001). Six weeks later `alpha.11`
added a schema check and found the same clause, independently, from the other
side. The registry's finding is recorded as **superseded rather than deleted**,
because the supersession is itself the result: a negative finding about an
instrument expires when the instrument improves.

Full detail is in the
[ecosystem register](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/01-ecosystem-context.md)
(row 1.5i) and the
[conformance strategy](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/03-conformance-strategy.md).
