<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Glossary

The terms this book and the tools' output use, in the sense they use them.

**Agreement check.** The project's calibration: on every CI run the official
conformance suite drives the reference server and host, the sessions are recorded,
and the validator's verdicts on them are compared with the suite's. A disagreement
nobody has explained fails the build.

**Capture.** Recording a session as a trace. `mcp-trace-capture` does it for any
implementation; the reference server and host can also record their own sessions.

**Capability gate.** The capability a clause depends on, such as `server.tools`.
A gated clause applies only to a session that negotiated that capability; in a
session that did not, it is *not applicable*.

**Check.** One mechanical test in the validator, with an ID such as
`base.request-id-unique`. A clause is judged by one or more checks. Every check is
shown to fail on at least one committed violation trace.

**Clause** (or **requirement**). One normative sentence of the specification —
a MUST, MUST NOT, SHOULD or SHOULD NOT — entered in the registry under a stable ID
such as `BASE-003`. The book's [reference pages](reference/requirements-2026-07-28.md)
list every one.

**Corpus.** The committed traces the validator is tested against: conforming
sessions, one violating trace per check, and recordings of real sessions, each
with a golden report. See [the corpus chapter](corpus.md).

**Event.** One line of a trace: a protocol message, an HTTP exchange's metadata,
or a transport opening or closing. See [the trace format](trace-format.md).

**Excluded.** A clause no recorded trace can judge — it is about an
implementation's internals, its user interface, or something that leaves nothing
on the wire. The registry gives the reason for each.

**Finding.** One concrete violation, addressed to a clause and, where possible, to
the event (`seq`) that broke it.

**Golden report.** The committed expected report for a corpus trace. A change in
the validator's output on any corpus trace is a test failure until the golden is
regenerated and the difference reviewed.

**Level.** A clause's RFC 2119 strength. A broken MUST or MUST NOT is a *fail*; an
unfollowed SHOULD or SHOULD NOT is a *warn*, which fails the run only under
`--strict`.

**Not applicable.** The outcome of a capability-gated clause in a session that
never negotiated the capability.

**Not observed.** The outcome of a clause whose subject never appeared in the
session — nothing was paginated, no error was returned. It is not a pass: the
session had no opportunity to break it.

**Registry.** The specification as data: every clause of a revision with its ID,
level, the party it binds, its verbatim quote and either its checks or its
exclusion. The **registry set** holds one registry per revision.

**Revision.** A dated version of the MCP specification: `2026-07-28` (current) or
`2025-11-25`. The validator judges the revision a session declares, unless
`--revision` names one.

**Reference server, reference host.** `mcp-everything-server` and
`mcp-reference-host`: an MCP server and client built on the official Rust SDK,
which pass the official suite and are used to calibrate the validator.

**`seq`.** An event's position in its trace, assigned by the recorder when it
observed the event. It is the only ordering the validator uses.

**Session.** One conversation between a client and a server, from opening to
closing. The validator judges each trace as one session.

**Trace.** A recorded session in the trace format: JSON Lines, one event per line.

**Verdict.** A report's conclusion: *pass*, *pass with warnings*, *fail*, or
*unsupported* (the build cannot judge a clause its registry names — a mismatched
installation). The exit status carries it.
