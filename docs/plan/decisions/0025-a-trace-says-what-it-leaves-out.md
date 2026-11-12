<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0025: A Trace Says What It Leaves Out

**Date:** 2026-11-12
**Status:** Accepted (maintainer decision D1 of the response to the 2026-11-11 audit, finding A02)
**Author:** Tom F. (maintainer); drafted by Claude Code

---

## Context

`mcp-trace-capture` forwards what it cannot hold rather than break the session it taps
([ADR-0019](0019-a-recording-tap-is-not-a-gateway.md)): a message over `--max-message-bytes`,
a body that arrives while its buffer budget is spent, an SSE event that is not a `message`.
Each was counted, and said on stderr at exit. The trace itself said nothing, so the only
durable verdict on such a session was the validator's `pass`, and the capture tool's exit
code was 0.

The audit's A02 was re-run against the binaries at triage time (commit `8435f7e`):

- Over HTTP a truncated `200` body (`{"jsonrpc":"2.0","id":3,"result":{`) reached the client,
  was absent from the trace, and the validator read `pass` with a note that a request had no
  response. The same bytes on a stdio line are recorded as a string payload and fail
  `BASE-008`/`TRAN-004`.
- A stdio line over the message limit was forwarded, absent from the trace, and judged `pass`.
- The capture README's exit table said `0` meant "`http`: stopped with a complete trace".

Two things are different in kind. A malformed body is *evidence*: it is a defect of the
session, and the trace should hold it. A message over the limit is *absent*: no trace can
hold it, and the trace should say that it is missing. The first is a verdict decision; the
second is a completeness statement. `Verdict::Incomplete` exists only for a run over several
traces, and a new enumerated value would be a breaking change to the report
([STABILITY.md](../../../STABILITY.md)).

## Decision

1. **A body that is not JSON is recorded as the text that was sent**, over HTTP as over stdio:
   a request body, a `2xx` response body and an SSE `message` event that is not JSON, or is
   JSON the trace cannot hold as sent. An error response (not `2xx`) is recorded that way
   only when its `Content-Type` says it is JSON; an HTML `404` page is not a message. This
   is a declared verdict change: such a session now fails `BASE-008` (`BASE-042` at
   `2026-07-28`) where it passed.
2. **What the trace still cannot hold is counted in the trace.** The closing lifecycle event
   carries an optional `capture` member, `{"unrecorded": {kind: count}}`, written by the
   recorder from what it was told and what it refused for length itself, per trace (a
   `{session}` trace counts its own). The kinds are the capture tool's own names
   (`oversized`, `over_budget`, `other_events`), and a reader accepts a kind it does not
   know. An empty `unrecorded` says nothing was left out as of the close; no member says
   nothing either way.
3. **The note is a note, never a verdict.** The validator reports it in every format
   (`capture` in the JSON report, a `NOTE` in text, an aside in HTML, a suite property in
   `JUnit`, an invocation notification in SARIF) when it reports a loss, and moves no total,
   verdict or exit status. It is the recorder's own statement and nothing authenticates it.
4. **Two opt-in gates, default exit codes unchanged.** `validate --require-complete-capture`
   exits `2` for a trace with no note or a note that reports a loss, the report still
   written. `mcp-trace-capture --fail-on-unrecorded` exits `3` in place of `0` for a run
   whose trace leaves anything out, or from which the proxy refused a request. The note
   cannot count what follows the close, or what the proxy refused before any session was
   known; those are on stderr and in the second gate.
5. **`Recorder::close` makes no claim.** A caller that reports everything its trace leaves
   out uses `Recorder::close_with_note`; the plain close writes no note, so a recorder that
   was never told to report cannot assert a completeness nobody checked.

## Consequences

### Positive

- The account of a capture's gaps outlives the terminal it was printed on: it travels with
  the trace through the workbench library, an export and a CI artifact.
- A malformed body over HTTP is judged, as it always was over stdio, instead of being hidden
  by the tool that was meant to record it.
- Nothing in the report's verdict changes for a trace without a note, and the 0.6.0 reader
  reads a trace with one: its `TraceEvent` has no `deny_unknown_fields` and its tests pin
  that an unknown member is ignored (read at the `v0.6.0` tag), and the trace schema is
  open. An older tool that re-serialises such a trace drops the member.

### Negative

- A verdict change for any HTTP capture whose server returns a non-JSON `2xx` body (a text
  body on a `202`, say): it now fails. That is the finding the old behaviour hid, and it is
  declared in the changelog.
- The note is unauthenticated and the gate trusts it: a forged note reads as complete. The
  gate defends against an honest recorder's silence, not against a trace edited to lie.
- A capture killed before its close, or whose write failed, has no note, so
  `--require-complete-capture` fails it. That is the intended reading of "no note", but it
  means a clean capture cut off at the last moment is refused.
- `TraceEvent` gains a `capture` field that is meaningful only on lifecycle events; the
  alternative (a field on the lifecycle variant) would have broken every pattern match on it.

## Alternatives Considered

- **A new `capture-gap` event at the position of each loss.** Rejected: exact positions are
  nicer, but an older validator rejects an event kind it does not know, so a trace with one
  would be unreadable by the releases that exist, and the position of an event the recorder
  never saw is not knowable beyond "between these two".
- **A sidecar `<trace>.capture.json`.** Rejected: the part of a recording that gets lost is
  the part that lives beside it, and it is the thing A02 complained of, moved to another
  file.
- **Reuse `Verdict::Incomplete` for one report.** Rejected: it would widen an enumerated
  value (breaking) and turn a statement about the capture into a statement about the
  session's conformance.
- **Fix only the README's exit table.** Rejected as the whole answer: it makes the overclaim
  true to read and changes nothing a pipeline can act on.
