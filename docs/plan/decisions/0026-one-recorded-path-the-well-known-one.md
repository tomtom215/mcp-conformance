<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0026: One Recorded Path — the Well-Known One, Set Apart from the Session

**Date:** 2026-11-21
**Status:** Accepted (by the maintainer, 2026-11-21; implemented the same day)
**Author:** Tom F. (maintainer); drafted by Claude Code

---

## Context

A recording held no request target at all. That was a line of the security model
([05-security-model.md](../05-security-model.md), "Trace corpora"): a path or a query is
where a misplaced token sits, so neither was written. It also made eight authorization
clauses unjudgeable, because the document they are about, a server's OAuth 2.0
Protected Resource Metadata (RFC 9728), is not served on the MCP endpoint, and a fetch
of it that did pass through a recorder could not be told from any other `GET`.

The 2026-11-20 classification of the exclusions found six of the eight judgeable given
two facts in the trace: which path was asked for, and what JSON came back. The other
two (`AUTH-012`, `AUTH-144`) need the MCP endpoint's own path, to say that a server
offers neither discovery mechanism.

## Decision

1. **A recorder keeps a request path only when the path the server is asked for starts
   with `/.well-known/`** and names something there
   (`mcp_conformance_core::trace::records_path`). RFC 8615 reserves that prefix for
   public metadata. The path is written on the request's `http` event and on its
   answer's. No query is ever written, and no other path: the MCP endpoint's own stays
   unrecorded, so `AUTH-012` and `AUTH-144` stay excluded.
2. **The JSON a well-known URI answers with is the `document` of the answer's `http`
   event**, not a `message` event. A message is something the protocol said; a metadata
   document is not, and recorded as one it failed the JSON-RPC clauses.
3. **An event that carries a `path` is not MCP traffic.** `engine::validate` takes such
   events out before it reads the trace as a session (`mcp_trace_validator::fetches`)
   and hands them to the checks that judge the documents. Recorded as an ordinary
   `GET` with its JSON as a message, a fetch failed six clauses and the wire schema on
   a conforming server: `BASE-008`, `LIFE-018`, `TRAN-013`, `TRAN-017`, `TRAN-039` and
   `TRAN-040` (measured on `corpus/good/http-invalid-token-refused-401.jsonl` with a
   fetch appended, before this rule and the last existed).
4. **`mcp-conformance check` makes the fetch**, for a server that answers `401`: the URL
   the challenge names as `resource_metadata`, or, when it names none, the well-known
   locations for the endpoint and then for the host. It sends none of the user's
   headers on that request, so a credential meant for the endpoint is not presented to
   another path, and it fetches only a URL on the origin it was given.
5. **Two checks**: the URL a `Bearer` challenge names serves a JSON object (`AUTH-006`,
   `AUTH-010`, `AUTH-091`, `AUTH-141`), and a returned document names at least one
   authorization server (`AUTH-011`, `AUTH-142`).

## What makes this sound, and what would falsify it

- **Nothing else is judged differently.** `tests/fetch_invariance.rs` inserts a fetch
  and its answer at three places in every corpus trace, after the first record, in the
  middle and at the end, and requires every row but the six to be unchanged; a control
  strips the `path` and requires the judgement to change. A corpus trace on which a
  fetch moves another row falsifies decision 3.
- **A fetch is tied to a challenge by path alone.** The recorder writes no host for it,
  because under the default `Host` handling a loopback server sees the proxy's address.
  The premise is that a recording is of one server. A recording assembled from two
  servers that serve different documents at one path would falsify a judgement made
  under decision 5; `check` cannot produce one, since it fetches only on the origin it
  was given.
- **The path is public by construction.** A server that puts a secret in a path under
  `/.well-known/` has published it already; the query, where a token is more plausibly
  misplaced, is dropped before the path is tested.

## Consequences

- `EventBody::Http` gains `path` and `document` (a breaking change to a public enum
  variant, declared in the changelog), and `trace-event.schema.json` describes both.
  A reader that ignores unknown members still reads every trace; one that judges
  `http` events must skip those with a `path`, or use `fetches::split`.
- Behind `mcp-trace-capture http` with an upstream URL that has a path of its own, the
  proxy's `/.well-known/…` reaches a path under the endpoint's, which is not a
  well-known URI and is not recorded. The fetch is recorded only where the upstream is
  an origin, as it is under `check`.
- Six clauses move from excluded to judged: `2025-11-25` judges 123 of 348, and
  `2026-07-28` 201 of 501.

## Alternatives considered

- **Record every path.** Rejected: it is the exposure the security model's line
  existed to prevent, for no clause that the narrower rule does not already serve
  (`AUTH-012`/`AUTH-144` aside, which do not justify it).
- **Record the document as a `message` and teach every check to skip it.** Rejected:
  measured, it failed six clauses (decision 3), and the next check written would have
  to remember.
  Taking the events out once, before any check runs, cannot be forgotten.
- **Fetch the document outside the recording and judge it apart.** Rejected: the
  verdict would rest on something the trace does not hold, and a trace is the whole of
  what this project judges.
