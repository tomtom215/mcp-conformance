<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0023: Extensions Are Reported, Never Scored — the Tasks and Skills Registries

**Date:** 2026-10-13
**Status:** Accepted (implements item 2.6 of the
[2026-09-24 review](../../reports/project-review-2026-09-24.md))
**Author:** Tom F.

---

## Context

Review item 2.6 asks for "extensions as separately-scored registries (tasks, skills),
mirroring the suite's `not_scored: extension` semantics", done when "extension rows
[are] reported but never affecting the core verdict; corpus pairs per check". What the
primary sources say, read for this decision on 2026-10-13:

1. **An extension is optional by definition, and is not published with the revision.**
   The site's extensions overview (`docs/extensions/overview.mdx`): "Extensions are
   always disabled by default and require explicit opt-in from the developer", and
   "SDKs can choose to implement extensions, but it's not required for protocol
   conformance." The site's pages for tasks (`extensions/tasks/overview`) and skills
   (`extensions/skills/overview`) defer to each extension's own repository for the
   normative text: `modelcontextprotocol/ext-tasks` (`specification/2026-07-28/tasks.md`,
   41 MUST-family, 15 SHOULD-family and 15 MAY-family keywords; read at `899c1bb`) and
   `modelcontextprotocol/ext-skills` (`specification/stable/skills.mdx`, "the source of
   truth", 80, 22 and 19; read at `167da6c`). Both are written against base revision
   `2026-07-28`; neither appears under `docs/specification/<revision>/`, so the page-set
   gate, which reads only that tree, never saw them.
2. **At `2025-11-25` tasks is not an extension.** It is a core page,
   `basic/utilities/tasks`, introduced "in version 2025-11-25 of the MCP specification
   and … currently considered **experimental**". That revision's `sources.json` keeps it
   out of scope, and the suite agrees there is nothing to score: SEP-1730 says
   "Experimental features and protocol extensions (such as Tasks and MCP Apps) are not
   required for any tier", and the suite's `requirements/2025-11-25.yaml` notes "The tasks
   extension suite attaches to 2026-07-28 only and has no entries here." Skills has no
   `2025-11-25` form at all.
3. **The suite's `not_scored: extension` exists only on its alpha line.** The stable pin,
   `@modelcontextprotocol/conformance@0.1.16`, has no `not_scored` anywhere — zero
   occurrences in its `dist/index.js` and README, and no `requirements/` directory. The
   draft pin, `0.2.0-alpha.12`, defines it in `requirements/2026-07-28.yaml`: "`not_scored`
   is run and reported but never counts toward a pass rate; each entry says why.
   `extension` is optional by definition", with the ten `tasks-*` scenarios entered
   `reason: extension`; its README adds "Both still run, so a failing extension stays
   visible in the report; neither moves the pass rate", and "The exit code follows the
   same rule: it reflects required scenarios only". Its runner marks an extension
   scenario by `source: {extensionId: …}`, treats such a scenario as applicable at no
   dated revision, and runs it on the `2026-07-28` wire. Its four skills scenarios —
   three `sep-2640-skills-*` server scenarios and `sep-2640-client-no-prefetch` — carry
   the same marker and appear in no requirements file.
4. **What a recording shows.** A tasks session is wire traffic — `resultType: "task"`
   results, `tasks/get|update|cancel`, `notifications/tasks` on a listen stream — and so
   is a skills server's half: its declaration, `skills/list` and `skills/get` results and
   errors, its manifests, `resources/read` of a `SKILL.md`, `resources/directory/read`.
   Most of the skills page binds the *host* — its registry, approvals, cache, trust
   model — none of which crosses the connection.
5. **Two core checks could not read a conforming extension session.** Recordings using
   the extension broke the core verdict: SUBS-001 reported `notifications/tasks` on a
   listen stream that asked for it through the extension's `taskIds` filter field, and
   TOOL-040 held a `CreateTaskResult` — a task handle — to the tool's `outputSchema`.

## Decision

1. **An extension's clauses are registry entries that name their extension.** A
   `Requirement` gains an optional `extension` member (`io.modelcontextprotocol/tasks`);
   entries carrying it live in `registry/2026-07-28/extensions/` and are in force at
   `2026-07-28` only. `Registry` keeps them apart from the core list —
   `Registry::requirements()` is unchanged, `Registry::extension_requirements()` holds
   them — so every reader of the core clauses (totals, the coverage table, the exclusion
   ledgers, calibration) is unaffected by construction rather than by care.
2. **They are judged like any clause and reported in a block of their own.** A report
   gains `extensions`: one entry per extension the session exercised, with its own
   `totals` and a `verdict` derived from them. **The report's `totals`, `verdict` and exit
   status never read it.** An extension appears once one of its rows passed, failed,
   warned or is unsupported; one the session never touched — the usual case for an
   optional extension — has nothing to report and is absent, so no core-only report
   changes. Every check behind an extension clause counts only extension traffic as a
   subject, which is what keeps that rule honest.
3. **Each format shows them and fails nothing over them.** Human output prints a block
   per extension ("not scored: its outcomes never change the verdict") before the
   report's totals. JUnit gives each extension a suite of its own, `… (revision,
   extension ID, not scored)`, whose findings are in `<system-out>` of passing cases —
   never a `<failure>`, `--strict` or not. SARIF reports them at level `note` under their
   clause's rule, with `properties.extension` and `properties.scored: false`. HTML gives
   them a section, linked from the contents and from the timeline. `report.schema.json`
   describes the block; `source.url` may now name the extension's own publication.
4. **The opt-in is the protocol's own.** No flag turns extensions on: a session uses an
   extension by negotiating or sending it, and that is when its block appears. A reader
   who implements an extension and wants it gated reads the extension's `verdict` from
   the JSON — the "deliberate promotion" the suite describes for its own sets.
5. **Every MUST- and SHOULD-family keyword of each extension's specification is entered**,
   judged where a recording can show it and excluded with the reason where it cannot:
   49 entries for tasks (25 judged by 18 checks) and 99 for skills (34 judged by 31
   checks). `registry/2026-07-28/extensions/sources.json` is the extensions' page set:
   each extension's site page, text location, in-scope and out-of-scope specification
   files and census, and every other page the site lists under `extensions/` with the
   reason it has no entries. `cargo xtask spec-drift` holds it to the site navigation,
   re-verifies the quotes, recounts the census, requires every keyword entered and
   resolves the tasks anchors against the extension's published site; `cargo xtask gates`
   checks its shape and its agreement with the entries offline.
6. **The tasks page at `2025-11-25` stays out of scope**, now for the reason fact 2 gives:
   experimental, unrequired for any tier, and absent from the suite's `2025-11-25` set.
7. **Core checks read extension traffic correctly.** SUBS-001 treats the extension's
   `taskIds` like `resourceSubscriptions`, a filter that requests per-item updates; and a
   `2026-07-28` `CreateTaskResult` is not the tool's result — the completed task's
   `result`, from `tasks/get`, is judged in its place, as `2025-11-25`'s `tasks/result`
   already was. Neither change moves an existing golden; both were found because the
   extension's corpus failed the core verdict for a conforming session.

## Consequences

### Positive

- The completeness claim extends to the two extensions the suite and SEP-1730 single
  out: an added or reworded clause in either fails the weekly census.
- A tasks or skills implementation gets clause-level findings on its recordings, with
  the same evidence model as the core, and no core verdict or CI gate moves because of
  them.
- The page-set gate now notices an official extension the site adds: a new page under
  `extensions/` fails `spec-drift` until it is entered or reasoned out.
- Two latent core misjudgements of conforming extension sessions are fixed.

### Negative

- **The skills specification has no anchored publication.** Its documentation site is
  not deployed (`modelcontextprotocol.github.io/ext-skills/…` answers 404) and the
  source of truth is an `.mdx` file, so skills sections name the file and links open it
  at the top. The tasks extension's site serves heading ids, and its anchors are
  verified like the core's.
- **The extension page sets are held to the site's navigation, not to the extension
  repositories' directory listings**, which need the GitHub API this gate avoids: a new
  specification file in an extension repository is noticed when the site page or an
  in-scope file changes, not on its own. Every out-of-scope file is fetched, so a
  reason cannot outlive its file.
- **Much of each extension is excluded.** 65 of the 99 skills entries are, most of them
  binding the host's own handling of skill content; 24 of the 49 tasks entries are, most
  binding timing, server state or client presentation. That is the honest size of what a
  recording shows.
- **A manifest's digests and sizes are not compared with the bytes a later read returns**,
  because the page itself names a skill updated after its entry was fetched as an
  ordinary cause of a mismatch. The frontmatter check is judged only where the bytes
  read are the bytes the manifest digests, and frontmatter is read by its top-level keys
  and plain scalars — there is no YAML parser — so a difference nested inside a value
  goes unjudged.
- **MCP Apps, the third official extension, is deferred, not decided**
  (`mcp-apps-extension` in `deferrals.json`); the authorization extensions are reasoned
  out, their clauses binding exchanges off the MCP endpoint.
- The report JSON gains a member and SARIF a level; consumers that reject unknown
  members, or treat every SARIF result as an error, see the extension's findings where
  they did not before.

## Alternatives Considered

- **Extension rows in the core list, marked `scored: false` and left out of the totals.**
  Rejected: every reader of `requirements` — the totals, the coverage table, the ledgers,
  the agreement check, third-party consumers summing rows — would have to learn to skip
  them, and one that did not would count them. Keeping them apart makes the separation a
  property of the data rather than of each reader's care.
- **A registry of its own per extension, judged and reported as a separate report** (a
  pseudo-revision such as `2026-07-28+tasks`). Rejected: an extension is judged on the
  same session, by the same pairing, against the same wire as its revision, and a second
  report would make every consumer correlate two documents for one trace; a revision
  string that is not a revision would also break `ProtocolRevision`'s contract.
- **Score an extension when the session negotiates it** (or behind a `--score-extension`
  flag). Rejected for now: SEP-1730 and the suite both keep extensions out of every
  pass rate, and a flag would add CLI surface whose effect the JSON's per-extension
  verdict already gives a caller who wants it. If implementers ask for an exit status,
  that is its own decision.
