<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Reading a report

A report answers one question per clause of the revision: did this session break
it? [Getting started](getting-started.md#reading-a-report) lists the seven outcomes;
this chapter is about acting on them.

## The outcomes, and what to do about each

| Outcome | What to do |
|---------|------------|
| `fail` | A MUST or MUST NOT was broken. Triage each finding (below). |
| `warn` | A SHOULD or SHOULD NOT was not followed. Decide whether the deviation is deliberate; `--strict` makes warnings fail the run. |
| `pass` | Nothing. The session carried traffic the clause binds to, and no check found a fault in it. |
| `not observed` | Nothing is wrong, and nothing was tested: the session never did what the clause is about. Raise coverage (below) if the clause matters to you. |
| `not applicable` | The clause depends on a capability this session did not negotiate. A session that negotiates it judges the clause. |
| `excluded` | No recording can judge the clause. `--all`, or `mcp-trace-validator requirements <ID>`, prints the registry's reason, which often says where the clause *is* tested. |
| `unsupported` | The installation is mismatched: the registry names a check this build does not have. Reinstall; the run exits `2`. |

Over several traces, a trace that could not be judged at all — unreadable,
malformed, of a revision this build does not know, or judging nothing — is listed
with its error, and the run's verdict is `incomplete`.

## Triaging a finding

```text
  FAIL  BASE-003 (MUST NOT)
        seq 3: request "tools/list" reuses id 1, already used by the same party at seq 0
        spec: "The request ID MUST NOT have been previously used by the requestor within the same session."
        see:  https://modelcontextprotocol.io/specification/2025-11-25/basic#requests
```

1. **Find the event.** `seq 3` is the event whose `seq` field is `3` — in a trace
   written by `mcp-trace-capture`, the line after `seq 2`. Many findings name a
   second event (`at seq 0`): the earlier one the offending event conflicts with.
   `--format sarif` puts each finding on its trace line in a SARIF viewer.
2. **Find the party.** The event's `direction` says who sent it
   (`client-to-server` or `server-to-client`). The clause says who it binds:
   `mcp-trace-validator requirements BASE-003 --revision 2025-11-25` prints the level,
   the party ("binding client and server"), the clause and its checks. Fix the side that sent the
   offending event.
3. **Read the clause where it is published.** The `spec:` line is the registry's
   verbatim quote, and `see:` links the sentence in the revision the session was
   judged against. Read the surrounding section before changing code: the clause may
   carry conditions the quote alone does not.
4. **Check the revision line** at the top of the report (`revision 2025-11-25
   (declared by the trace)`). A session judged against a revision it did not mean to
   speak fails clauses it never agreed to; `--revision` names the right one.

A few findings come from the recording rather than the implementation: two sessions
in one trace (the validator notes this, and `{session}` in the capture's `-o`
separates them), or a capture started mid-session. [Troubleshooting](troubleshooting.md)
covers both.

## The wire schema

Below the clause rows, every report judges each message against the revision's own
JSON Schema — the specification's `schema.json`, which it calls "the source of truth
for all protocol messages and structures":

```text
  FAIL  wire schema — fail (1 of 5 messages invalid)
        seq 4: the server's message is not a valid ListResourcesResult under the 2025-11-25 schema: /result/resources/0: "uri" is a required property
        spec: "This is the source of truth for all protocol messages and structures."
        see:  https://modelcontextprotocol.io/specification/2025-11-25/basic#schema
        schema: https://github.com/modelcontextprotocol/modelcontextprotocol/blob/<commit>/schema/2025-11-25/schema.json
```

The finding names the definition the message was held to and the path of the member
at fault; a member that matched none of several alternatives (a content block, an
input request) names the closest alternative's defect. The schema catches what no
sentence of the prose states on its own — a missing required member, a wrong type, a
value outside an enumeration — so it is reported beside the clauses rather than as one,
has no requirement ID, and is not counted in the totals; a failure still fails the
verdict. Where the published schema is stricter than `schema.ts`, the validator
corrects it and says where
([ADR-0020](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0020-wire-schema-validation.md)).

## Why so many clauses are not observed

A pass verdict says that nothing *this session* did broke a clause. A short session
does little: one `initialize` and a `tools/list` says nothing about pagination,
errors, resources, cancellation, or the HTTP transport. Those clauses report `not
observed`, and the totals count them so that a thin session cannot read as a
thorough one.

To put more of the specification to the test, record sessions that do more:

- exercise every feature the implementation offers — tools, prompts, resources,
  completion, subscriptions;
- include error paths: an unknown tool, a missing resource, invalid parameters;
- page through a long list, cancel a request, ask for progress and logs;
- record over both transports, since Streamable HTTP has its own clauses;
- for a server, let `mcp-reference-host` drive it with `--sweep` (and `--cancel`,
  `--subscribe`, `--log-level` at `2026-07-28`) — see
  [Capturing from your client](clients.md#no-client-at-all).

`validate traces/*.jsonl` judges each recording separately: coverage is per trace,
not pooled across them.

## Disputing a verdict

If a finding disagrees with your reading of the specification, open an
[issue](https://github.com/tomtom215/mcp-conformance/issues) with the requirement ID,
the specification text, and — if you can share it — the trace (traces record message
content in full; read one before attaching it). Disputes are triaged into one of
three outcomes: a bug here (fixed), a divergence from the official conformance suite
(filed upstream), or an ambiguity in the specification (filed upstream). The
[contributing guide](https://github.com/tomtom215/mcp-conformance/blob/main/CONTRIBUTING.md#reporting-verdict-disputes)
has the process.

## The report as a page

`--format html` writes the report as one HTML file — the one to attach to a bug report
against an SDK, or to open while debugging:

```text
mcp-trace-validator validate session.jsonl --format html > report.html
```

From the top: the verdict, with every outcome's count (one card per revision when the trace
is judged at several) and the wire-schema outcome; then **Clauses to act on**, each failing,
warning or unsupported clause with its level, the clause verbatim, a link to its published
page, and every finding with the `seq` it points at; then the **Message timeline**, every
event in capture order with its direction, kind, method and id, the request a response
answers (both linked), HTTP requests and responses with their recorded headers, and each
finding shown at its event. A finding links to its event and the event back to the finding,
and the switch above the timeline hides the events no finding names. **Accounting** closes
the page: the passed, not-observed, not-applicable and excluded clauses, collapsed, each
excluded one with the registry's reason and each not-applicable one with the capability the
session never declared — the same arithmetic as the totals, clause by clause.

Anchors are names you can type: `report.html#BASE-003` opens a clause, `#e12` an event
(`#2026-07-28-BASE-045` when two revisions share a page, `#t1-e12` in a page over several
traces).

The page is safe to open from anyone. It loads nothing — no script, font or image — and the
only style it applies is its own, which its Content Security Policy admits by digest;
everything from the trace (method names, ids, headers, payloads) is shown as text, so a
recording holding `<script>` shows the word. It is still a trace: payloads are shown in
full up to 16 KiB per message, so read it before you share it, as you would the trace.

The page is for people. Its layout changes between releases; for anything a script reads,
use `--format json`.

## The report as data

`--format json` is the whole report, described by its
[JSON Schema](https://github.com/tomtom215/mcp-conformance/blob/main/crates/mcp-trace-validator/schema/report.schema.json).
A single trace gives one object: `revision`, `revision_source` (how the revision was
chosen), `totals` per outcome, `verdict`, and `requirements`, one row per clause
with its `id`, `level` and `outcome`. A failing or warning row also carries `source`
(the quote, its section and URL) and `findings`, each with the `check` that found it,
the `seq` of the event, and a `detail`. Several traces, or several `--revision`s, give
the multi-trace and multi-revision shapes the schema also describes.

The findings alone, for a script:

```text
mcp-trace-validator validate --format json session.jsonl \
  | jq '.requirements[] | select(.outcome == "fail" or .outcome == "warn")
        | {id, level, findings}'
```
