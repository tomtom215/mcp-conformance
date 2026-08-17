<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project are documented in this file.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0, minor releases may contain breaking changes; entries say so explicitly.

## [Unreleased]

### Added

- **The `2026-07-28` registry is complete: all fifteen in-scope pages, 272
  entries, nothing aspirational.** Every page is entered by the same
  per-requirement method as `2025-11-25` — live fetch, verbatim quote, then a
  named check or an exclusion whose reason is specific to that clause — giving
  **272 entries: 124 judged, 0 unsupported, 148 excluded**, behind the
  off-by-default `draft-2026-07-28` feature. `spec-drift` verifies **412 quotes
  across both revisions**, and every judged clause has both a conforming and a
  violating trace behind it. The `2025-11-25` registry is untouched at 140
  entries, and a test asserts the byte-equality rather than assuming it.

  Seventy-one checks are new, all feature-gated, covering the revision's new
  machinery as well as its restatements: the stateless `_meta` envelope, the
  error-code partition, Streamable HTTP's header and rejection rules, stdio's
  notification-based cancellation, `server/discover`, version negotiation and
  extension identifiers, Multi Round-Trip Requests, `subscriptions/listen`,
  caching hints, per-request logging, and this revision's capability
  declarations. Judging Base64 sentinel values needed a decoder; it is ~25 lines
  beside the existing validator rather than a new dependency, so the judgment
  surface stays `serde`-only.

  Boundaries are the specification's, not ours: notification POSTs are unjudged
  because the revision states their header requirements are undefined, and the
  clauses needing a JSON Schema engine stay excluded. The 148 exclusions
  concentrate where a recording genuinely cannot reach — `requestState` is
  opaque by design, a cache hit puts nothing on the wire, elapsed time is not
  available to checks, host-OS actions are outside the trace vocabulary, and
  classifying whether a payload holds a credential or PII is not a structural
  question.

  Each check carries unit tests alongside its corpus pair, because the two
  answer different questions — "does this check ever fire?" and "does it fire on
  exactly the right thing?" — and only the second scales with the number of
  branches a check has.

- **The `2026-07-28` corpus is held to the shipped corpus's contract.** Its
  violation traces are now name-attributed (each must falsify the requirement it
  is named after) and byte-pinned against goldens in `corpus/golden/draft/`,
  matching `corpus/violations/`. Previously they were covered only by "some
  trace kills each check", which cannot see a finding that has drifted onto a
  neighbouring requirement.

- **`mcp-everything-server` serves the `2026-07-28` stateless surface, over
  both transports.** `--protocol-version 2026-07-28` selects it: no
  `initialize`, no sessions, `server/discover` for capability advertisement,
  per-request `_meta` required, and SEP-2549 caching hints on cacheable
  results. The official suite's `2026-07-28` scenarios pass **23/23** against
  it, and this workspace's own registry judges **124 clauses pass, 0 fail** on
  captured sessions over HTTP *and* over stdio.

  stdio needed the enforcement rmcp performs inside its HTTP tower layer and
  that a header-less transport has nobody to do: `StatelessEnvelope` supplies
  it, rejecting a request whose `_meta` lacks `protocolVersion` or
  `clientCapabilities` with `-32602` naming the field, and one naming a version
  this server does not serve with `-32022` carrying `data.supported`.

  Server-to-client requests go by **SEP-2322's MRTR pattern** at this revision:
  `elicitation/create` and `sampling/createMessage` are returned inside an
  `input_required` result and the client retries the original call with its
  answers, because the revision forbids sending them independently on either
  transport. `logging/setLevel` is gone with it — log notifications ride only
  requests whose `_meta` asked for them — and `test_url_elicitation` is not
  listed at all, its feature having been removed. The `2025-11-25` mechanisms
  are untouched and still score 40/40 on the pinned suite. The revision is chosen when the server is built (`ServedRevision`),
  so the `2025-11-25` surface has no new branch to take and is unchanged byte
  for byte — the `draft-readiness` baseline for that mode is identical to the
  one committed before this work.

  One piece was a correctness decision rather than configuration: `initialize`
  is **refused** with `-32022` carrying `data.supported`. rmcp's default
  negotiates an unsupported version down to the server's own and answers with a
  *result*, which would leave a legacy client believing it had connected to a
  server that then rejects every request it sends. The refusal is what VERS-001
  requires of a version refusal and the diagnostic VERS-008 asks a modern-only
  server to give.

  *Breaking (pre-1.0):* `http::router` and `http::router_tapped` take a
  `ServedRevision`. A conformance server's revision is the most important fact
  about it, so it is named at the call site rather than defaulted silently.

- **The `2026-07-28` corpus is no longer purely authored, and now has a
  control.** `corpus/draft/captured/` holds two real recordings — the official
  MCP conformance suite `0.2.0-alpha.9` driving its `2026-07-28` scenarios
  against both server modes, same client, same scenarios, same run — and the
  golden suite re-validates both on every run. An authored fixture can only
  confirm its author's reading of the specification; this is the cross-check
  that reading cannot provide, and the matched pair is what makes every
  difference between the two reports attributable to the one variable.

  The legacy server scores **123 pass, 1 fail** (CACH-001: no `ttlMs` on
  cacheable results, the correct answer for a server held to a revision it does
  not implement); the stateless server scores **124 pass, 0 fail**. The
  official runner scores both **23/23** — its scenarios exercise features and
  cannot separate the two servers, which is exactly the gap a requirement
  registry fills.

  A third capture, `reference-host-2026-07-28-stdio.jsonl`, records the same
  server over stdio (`cargo xtask draft-capture`). Its provenance is weaker and
  labelled so — the official suite drives servers over `--url` only, so both
  ends are this workspace's — but it is the capture that earned its keep: the
  first recording of it found **six real defects** in the stateless server that
  no HTTP capture could reach, because the suite's `2026-07-28` scenarios
  exercise no interactive tool and no logging tool. Five were the pre-MRTR
  mechanism (TRAN-060/066/119/120, MRTR-001) and one was logging without being
  asked (LOG-008).

- **`tools/extract-clauses.py --verify`** runs `spec-drift`'s comparison offline
  over a committed registry directory — the fast inner loop while curating a
  page, reproducing the network gate exactly.

### Fixed

- **The session trace tap could not record protocol revision `2026-07-28` at
  all, and said nothing.** It keyed every exchange on `Mcp-Session-Id` and
  returned early without one — and that revision removes the session concept
  outright (SEP-2575), so every exchange of it took that branch. The failure was
  silent: an empty trace directory, indistinguishable from a server nobody
  talked to. `cargo xtask draft-readiness` had been driving the official suite's
  `2026-07-28` scenarios against a tapped server and discarding every byte.
  Sessionless exchanges are now recorded to a per-run `stateless` trace; this
  also recovers `2025-11-25` exchanges that never formed a session (a rejected
  `initialize`, a request refused before one existed), which were being lost the
  same way.

- **`meta.missing-capability-error` demanded a shape the schema does not
  define.** BASE-035's quote says a `-32021` carries `data.requiredCapabilities`
  that "lists the missing capabilities", and the check read "lists" as a JSON
  array. The `2026-07-28` schema types the field as `ClientCapabilities` — the
  same nested object a client sends in its `_meta` — so a conforming server was
  reported for using it, which is the worst failure mode a conformance check
  has. Corrected against the schema; the array shape is now itself a violation
  fixture.

- **Three interactive tools resolved client capabilities through the
  handshake.** They read `peer.peer_info()`, which is `None` for every request
  on a stateless stdio server, so a client that declared `elicitation` in the
  `_meta` envelope the server was reading would have been refused anyway. They
  now read `RequestContext::client_capabilities`, which resolves the request
  first and falls back to session state only when a session exists — one call
  site, correct at both revisions. At `2026-07-28` the refusal is SEP-2021's
  `-32021` carrying `data.requiredCapabilities`; the `2025-11-25` refusal is
  unchanged.

- **The tap dropped the headers four `2026-07-28` clauses are judged by, and
  the validator reported the clauses they prove.** Its recording allowlist held
  seven header names, chosen when `2025-11-25` was the only revision it
  recorded, and did not include SEP-2243's `Mcp-Method`, `Mcp-Name` and
  `Mcp-Param-*` or SEP-2570's `X-Accel-Buffering`. Both sides of the recorded
  exchange were conforming and both were reported: rmcp's transport *rejects* a
  `2026-07-28` request that arrives without `Mcp-Method`, and rmcp sets
  `X-Accel-Buffering: no` on every SSE response it builds. TRAN-058 and
  TRAN-068 were findings about the recording, not about any implementation —
  and one of them had been written up in `corpus/README.md` as a defect in the
  official suite's client. The allowlist now carries those names plus a
  `mcp-param-` prefix arm, since those header names are chosen by a tool's
  `x-mcp-header` annotation and cannot be enumerated. Recording them widens
  nothing: every value is a copy of an argument already recorded verbatim in
  the body, and the prefix cannot match `authorization` or `cookie`.

- **Five capability checks would have reported vacuous passes at `2026-07-28`.**
  Every feature page states "Servers that support X MUST declare the X
  capability", and the `2025-11-25` checks for it resolve declarations through a
  helper that abstains unless the trace carries an `initialize` **result** — a
  message this revision does not have. Reused as-is, each would have inspected
  nothing and reported `pass`. `checks/draft/capabilities.rs` reads this
  revision's declaration surface, the `server/discover` result, instead.

- **`transport.unsupported-version-error` bundled an HTTP status into a rule
  stated without one.** TRAN-074 requires "400 Bad Request *and* an
  `UnsupportedProtocolVersionError` listing its supported versions", while
  `basic/versioning` states the same rule with no status; one check covering
  both would have reported a wrong status against a clause that never mentions
  statuses, since the engine attributes a finding to every requirement naming
  the check. Split into `transport.unsupported-version-{error,status}` — which
  exposed that the HTTP-400 half had never been falsified by any trace on its
  own, and now is. Its obligation side also read the requested version out of
  the POST rather than the request's own `_meta`, making the rule invisible on
  stdio, where `basic/versioning` states it just as plainly.

- **`meta.missing-required-field-rejected` reported a conforming server.** It
  requires `-32602` for a request missing `_meta` fields, and a legacy
  `initialize` reaching a modern server is missing them by definition — but
  `basic/versioning`'s compatibility matrix states that for exactly that
  exchange "the exact code is implementation-defined". Every cross-era capture
  would have carried a MUST failure the specification has waived. `initialize`
  is now outside that rule, by method; the client's own defect is still
  reported.

- **`transport.client-no-responses` filtered on Streamable HTTP**, so it would
  have been inert for the stdio clause stating the same rule. The revision
  removed server-initiated requests, so there is nothing on any binding for a
  client response to answer, and the filter is gone.

- **`meta.missing-required-field-http-status` and
  `meta.missing-capability-http-status` could not fail on a real capture.**
  Both ask whether a JSON-RPC error carried HTTP 400, and the helper they share
  searched *forward* from the error message for the next recorded status — but
  the tap records a response's `http` event **before** the message it framed,
  as every captured trace in `corpus/good/` shows. On a live recording the
  helper read the following exchange's status, or none at all, so both clauses
  reported a vacuous pass. It now scans backwards to the nearest server-sent
  status, which also handles SSE correctly: every frame of one response rides
  one status event.

- **A reused check would have reported a vacuous pass for TRAN-071.** The
  `2026-07-28` clause "every POST request MUST include an
  `MCP-Protocol-Version` header" was pointed at the `2025-11-25` check of the
  same purpose, which returns early unless it finds the version negotiated in
  an `initialize` result — the handshake this revision removes. It would have
  inspected nothing and passed, which is worse than the `unsupported` it
  replaced: an absent check is visible in the totals, a vacuous one is not.
  Caught before shipping; the clause now names a check written against the POST
  itself.

- **A header value shaped like the Base64 sentinel was treated as encoded even
  when its payload was not.** `=?base64?café?=` matched the sentinel pattern and
  was skipped, but it is not an encoded value — it is a header that still cannot
  be transmitted, which is exactly what the Value Encoding clauses forbid. Only
  a *miscased* sentinel is now deferred (to the marker-case clause); everything
  else is judged on the bytes it actually carries.

- **Five clauses were judged by checks that bundled their neighbours' rules.**
  The engine attributes a check's finding to every requirement naming it, so a
  trace carrying an unencoded non-ASCII header value reported the *marker-case*
  clause as failed. The two bundling checks are split into six along the rules
  they state; requirements now share a check only where they state one rule
  across several sections.

### Changed

- **Breaking (pre-1.0):** `mcp_everything_server::http::router` and
  `http::router_tapped` take a `ServedRevision`. Pass
  `ServedRevision::default()` (or `ServedRevision::V2025_11_25`) for the
  behaviour these functions had. The revision is named at the call site rather
  than defaulted silently because it is the single most important fact about a
  conformance server, and a reader of `router(policy)` could not tell which one
  was being served.

- **M2.5 Phase 0: the registry can describe two revisions, and the `2025-11-25`
  surface provably did not move.** All 140 embedded entries are now bounded
  `applies: {removed: "2026-07-28"}`, and `RegistrySet::builtin()` describes the
  new revision behind a new off-by-default `draft-2026-07-28` feature on
  `mcp-conformance-core`.

  The bound is the point. An absent `applies` range means *every* revision, so
  describing a second one without it would have silently applied all 140
  entries — every quote citing a `2025-11-25` page — to `2026-07-28`, making an
  unextracted revision read as fully covered. Two tests stand guard: the
  `2025-11-25` projection still reconstructs `Registry::builtin_2025_11_25()`
  byte-for-byte at 140 entries, and no embedded requirement applies at
  `2026-07-28`. Both run in each feature mode.

  With the feature on, `registry("2026-07-28")` answers with an **empty but
  real** registry — a state `RegistrySet::registry`'s contract already
  distinguished from `None`. It is off by default precisely because an empty
  registry judges nothing and therefore fails nothing; a default build must not
  be able to read that silence as conformance.

  No verdicts change: 40/40 server and 4/4 client conformance stay green with
  zero unexplained divergence, and `spec-drift` still verifies every quote.

- **Correction: the registry is 140 entries, not 130.** Several figures
  published earlier this session were computed by scripts that skipped
  `resources.json`, because `"resources.json".endswith("sources.json")` is true
  and the loops filtered the page manifest that way. The extractor's calibration
  claim survives re-checking at the true total — **140/140** quotes verify — but
  the counts in `tools/extract-clauses.py` and the extraction inventory are
  corrected, and the trap is documented in the tool. The true split, 52 checked
  / 88 excluded, matches what v0.4.0's changelog already stated.

- **The SDK moved from `rmcp 1.7.0` to `3.1.2`, and a before/after wire diff
  says what that changed.** ADR-0011 held the pin until both the `2026-07-28`
  text shipped and rmcp cut a stable `3.0.x`; both are now true, so the hold
  expired by its own terms. The 40/40 server and 4/4 client conformance gates
  stayed green with both expected-failure lists still empty, and the agreement
  check still reports zero unexplained divergence.

  The diff was calibrated before it was trusted: two consecutive runs at 1.7.0
  produced zero differences across all 34 sessions, so anything the upgrade
  moved is signal. Exactly three effects account for every difference — five
  `inputSchema`s and one `outputSchema` no longer carry the schemars-derived
  top-level `title`/`description` (rustdoc artifacts that were leaking onto the
  wire); `clientInfo.version` follows the SDK version, which is register 3.9's
  `from_build_env` bug re-observed rather than anything new; and the
  concurrent-multi-stream session became nondeterministic in its response
  interleaving, proven by diffing two 3.1.2 runs against each other. Normalize
  the first two away and 29/30 server sessions and 4/4 client sessions are
  byte-identical, the sole exception being the session shown to be
  nondeterministic. Register 3.16.

- **Tool-argument validation now reports the error class the spec asks for.**
  Arguments that fail a tool's own `inputSchema` are returned as a tool
  execution result with `isError: true`, not as an MCP protocol error. This is
  a **behaviour change, and a correction**: the `2025-11-25` tools page places
  "Input validation errors" under tool execution errors and reserves protocol
  errors for unknown tools, malformed `CallToolRequest`s, and server errors.
  rmcp changed this in 1.8.0 and this workspace pinned the old, non-conformant
  behaviour until now. Unknown tools stay `-32602`, and missing *prompt*
  arguments stay protocol errors because `GetPromptResult` has no `isError`
  channel. Three tests were inverted deliberately. Register 3.17.

- **URL-mode elicitation is preserved rather than dropped.** rmcp 3.x deleted
  `notifications/elicitation/complete` along with the `2026-07-28` removal of
  the feature, but the notification is still part of `2025-11-25`, which this
  server implements and rmcp still lists as supported. It is now sent and
  received through `ServerNotification::CustomNotification`, keeping the
  capability the crate advertises. The host deliberately matches the literal
  method name: rmcp 3.x's `ElicitationResponseNotificationMethod` constant is
  `notifications/elicitation/response`, a *different* notification, and binding
  to it would have left the host silently deaf — caught by the round-trip test.

- **Readiness for `2026-07-28` jumped from 1 passing / 20 failing / 1
  informational to 23 passing / 0 failing / 0 informational.** The single
  blocker was the removed handshake: rmcp 1.7.0's server rejected every
  scenario with HTTP 422 before any handler ran. On rmcp 3.x the everything
  server passes the official runner's whole `2026-07-28` scenario set.
  This remains **not** a conformance claim about that revision — the
  requirement registry still does not describe it and the validator is not
  involved — but the lifecycle blocker is gone. Baseline re-blessed.

- **SEP-2577 deprecation allows are module-scoped, never crate-wide.** Roots,
  Sampling and Logging are deprecated forward but remain required on the
  `2025-11-25` surface the suite grades, so rmcp 3.x's attributes fire on
  correct code. Each of the six library modules and two test modules carries
  its own `#![allow(deprecated)]` with a comment naming the feature. The honest
  cost, stated in each: an unrelated future deprecation in those modules would
  also be silenced.

- **`cargo-deny` gets a documented `base64` duplicate skip, because the rmcp
  upgrade created one.** rmcp 3.1.2 requires base64 0.23 while hyper-util
  0.1.20 (reached via both axum and reqwest) still requires 0.22, and `bans` is
  `deny`. This is stated in the skip as costing more than the neighbouring
  `syn` skip: syn is build-time only, whereas both base64 copies are compiled
  into the binaries. Accepted because neither version is ours to pin, with an
  exact pin so it re-fires when either moves. Found by CI, not locally —
  cargo-deny is now installed in the dev loop so `cargo xtask ci` reports
  "every local gate ran and passed" instead of skipping it.

- **`rmcp-macros` ceiling shim moved to `>=3.1.1, <3.2.0`.** Register 3.13 is
  still unfixed upstream — rmcp 3.1.2 caret-pins its own macro crate — so the
  shim keeps doing real work.

- **The `2026-07-28` specification shipped on its scheduled date, and the
  repository's claims about it are corrected to match.** v0.4.0 was cut one day
  ahead of the text and said so throughout; those statements are now false and
  a reader cannot tell which are history and which are current. Recorded as
  register row 1.5h with four independent confirmations of a real cut — the
  site default redirects to `/specification/2026-07-28`, the version dropdown
  marks it `(latest)`, upstream's `draft/changelog.mdx` was reset to its
  post-release stub, and `schema/2026-07-28/schema.ts` is on `main`. The
  shipped changelog carries **exactly the planned inventory** (nine Major,
  twelve Minor, four Deprecated, unchanged since 1.5d/1.5e), so roadmap M2.5's
  extraction checklist needs no re-scoping. Documentation only: no crate's
  behaviour, API, or verdicts change.
- **`spec-drift` resolves its pages per revision instead of hardcoding one.**
  Both the sources path and the raw base URL are now derived from a revision,
  so `2026-07-28` registry entries will be verified against the `2026-07-28`
  text rather than silently checked against `2025-11-25` pages. With one
  published revision this was a distinction without a difference; with two it
  is a correctness bug waiting for the next entry. The live gate still
  verifies all 140 quotes.
- **ADR-0011 amended (2026-07-28), decision unchanged.** Its first condition
  is met and its second is not: rmcp's newest is `3.0.0-beta.4` while
  `max_stable_version` is still `2.2.0`, so the pin holds at 1.7.0 and the
  `rmcp-macros` ceiling shim stays in force. The amendment also retires a
  supporting argument that expired with the release, and records that the
  Decision's bundling of the SDK upgrade with M2.5's extraction scopes *when
  the upgrade lands*, not when extraction may start.
- **`draft-readiness` prose corrected:** "draft" now describes the pre-release
  suite scenarios (`0.2.0-alpha`), not the specification. The task, its
  committed baseline filename, and the report path keep their names — those
  are load-bearing across CI — and the rename is deferred to the suite pin
  bump, where the pins move anyway. The committed `_policy` string matches the
  generator byte-for-byte, so `BLESS=1` produces no spurious diff.

## [0.4.0] - 2026-07-27

**Verdicts change in this release.** `PROM-008` moves from *excluded* to
*judged*, so a trace that previously reported no finding for it can now report
one, and every report's totals shift (52 checked / 88 excluded, was 51 / 89).
Pre-1.0 minor releases may do this and the README says so; this note is the
"explicitly" that promise refers to. Re-baseline any committed golden reports.

**The API grows, and nothing is removed.** `RegistrySet`, `AppliesRange`,
`Requirement::applies_to`, the `multi` module with `validate_revisions` /
`MultiReport`, the CLI's `--revision` and `--registry-set`, and the
off-by-default `draft-2026-07-28` feature are all additive; the single-revision
`Registry` and `engine::validate` are untouched. Code written against 0.3.0
compiles unchanged against 0.4.0.

**Why now, one day before the `2026-07-28` specification ships:** multi-revision
validation is the feature whose value peaks across the migration window, and it
is more useful in a published crate the day the text lands than in a repository
a week later. The registry content for the new revision is deliberately *not*
here — it is extracted from the final text, which had not shipped when this was
cut (`docs/plan/01-ecosystem-context.md` row 1.5f).

### Added

- **The release pipeline now proves the published crate is installable**
  (`verify-install` job in `release.yml`). `RELEASING.md` step 5 used to read
  "install path works on a clean machine" and leave it to a human, which is the
  one shape of claim this project otherwise refuses to leave un-gated. The job
  runs `cargo install mcp-trace-validator` — the exact command the README gives
  users — on an uncached runner on both stable and MSRV the moment `publish`
  finishes, then runs the installed binary and checks it reports the published
  version, and separately re-installs `--locked` to prove the lockfile shipped
  inside the crate still resolves. It is deliberately a **detector, not a
  gate**: by the time it can run, the version is immutable on crates.io. That
  is not a flaw in the design but a property of the problem — the bug it exists
  for is a packaging one, a file missing from the `.crate` that every
  pre-publish gate passes because the workspace still has it on disk, and it
  cannot run earlier because `mcp-trace-validator` depends on
  `mcp-conformance-core`, so installing it from the registry is impossible
  until the sibling is published. Finding that from CI in minutes beats finding
  it from a user's bug report.
- **A requirement moved from excluded to judged, by auditing the exclusions**
  (`PROM-008`; registry now **52 judged by 48 checks, 88 exclusions**). The first two
  registry audits re-read the *spec text* looking for clauses with no entry. This one
  re-read the 89 *exclusions* — the entries that claim a recorded trace cannot judge
  them — against what a trace actually carries. 88 held. `PROM-008` ("Servers SHOULD
  validate prompt arguments before processing") did not: it was excluded because
  "validation thoroughness is implementation-internal", which is true of thoroughness
  and false of failure. A server that answers `prompts/get` with a successful result,
  when the request omits an argument that same server published as `required` in its
  own `prompts/list`, demonstrably processed input it had declared invalid — and both
  halves of that comparison are recorded messages, needing no server-side knowledge.
  That is the same defect the second audit found in `TRAN-026`: an exclusion whose
  stated reason was simply untrue. The new `prompts.arguments-validated` check is
  deliberately narrow, because a verdict is only worth its soundness — it judges only
  prompts whose required arguments were observed being declared, and only when the
  server returned a *result*, since an error response means the server did reject the
  call. `PROM-007` stays excluded on purpose: two of its three enumerated failure cases
  (invalid prompt name, internal error) genuinely are server-side ground truth.
- **A measured readiness score for the next revision** (`cargo xtask draft-readiness`;
  scheduled `draft-readiness` CI job). The `2026-07-28` change inventory says *what*
  changes; it cannot say how large the migration is. This drives the official runner's
  **draft scenario set** against the current `2025-11-25` everything server and ratchets
  every check's status against a committed baseline
  (`conformance/draft-readiness.json`) — the gate fails when a passing check is lost
  (a migration regression) *and* when one is gained or the suite's check set moves
  (`BLESS=1` re-records), so the figure quoted in the roadmap cannot drift in either
  direction. Both the suite version and the spec revision are exact pins: a ratchet whose
  input floats is not a ratchet, which is also why this is a separate job from the
  alpha-tracking one whose whole purpose is to float. Statuses are recorded verbatim per
  check rather than folded into a passed/total ratio, because the runner's `INFO` outcome
  is neither a pass nor a failure and a denominator that absorbs it is how a conformance
  number becomes a lie — the same reason ADR-0006 reports capability-gated requirements as
  not-applicable. Deliberately separate from `cargo xtask conformance`: the runner here
  speaks a revision the registry does not describe, so there is nothing to reconcile the
  validator against, and the failures are findings rather than build breakage.
  **First measurement (2026-07-26): 1 passing, 20 failing, 1 informational across 20
  scenarios** — and every failure is the same failure, an HTTP 422 at the removed
  `initialize` handshake, so the migration is one piece of work (the stateless lifecycle)
  rather than twenty. The single pass is the DNS-rebinding rejection, which is
  revision-independent because the `Host`/`Origin` policy runs in middleware ahead of any
  protocol handling. Written up in
  `docs/reports/draft-2026-07-28-readiness-2026-07-26.md`.
- **Stateless `2026-07-28` lifecycle variant** (roadmap M2.5; behind a new, off-by-default
  `draft-2026-07-28` feature on `mcp-trace-validator`). A second session state-machine
  variant — `context::draft` — alongside the `2025-11-25` one, modelling SEP-2575's
  stateless rework: with the `initialize`/`initialized` handshake removed, a session is
  operational from its first message (no `BeforeInitialize`/`Ready` gate), and the only
  remaining handshake-like exchange is the optional one-shot `server/discover` probe
  (`Active` ⇄ `AwaitingDiscoverResult`, with its error edge). Every transition and the
  error edge are unit-tested, with a property test over arbitrary interleavings. Scoped to
  the lifecycle and built alongside — not wired into judgment — pending the final spec
  text; it tracks the draft SEPs (register 1.5a–1.5b) and must be reconciled against the
  `2026-07-28` text when it ships. Carries no new runtime dependencies.
- **Multi-revision trace validation** (roadmap M2.5 infrastructure). The registry format
  gains an `applies` revision range — the half-open `[introduced, removed)` interval
  ADR-0006 deferred until a second revision landed — exposed as `AppliesRange` and
  `Requirement::applies_to(revision)` (an absent range means "every revision", so every
  existing entry is unchanged). A new `RegistrySet` carries the union of requirements
  across revisions and projects to a single-revision `Registry` for any revision it
  describes, sharing one definition of "well-formed" with the single-revision loader.
  `mcp-trace-validator` gains `multi::validate_revisions`, which judges one trace against
  several revisions in a single pass and aligns the results into a `MultiReport`: one row
  per clause with its outcome under each revision, a clause *absent* from a revision (its
  `applies` range excludes it) kept distinct from ADR-0006's capability `not-applicable`.
  Exposed on the CLI as `validate --revision <YYYY-MM-DD>` (repeatable) with an optional
  `--registry-set`, in human and JSON form. The machinery is built and tested against the
  shipped `2025-11-25` as the sole built-in revision plus synthetic multi-revision data, so
  the `2026-07-28` entries drop in as data behind the planned `draft-2026-07-28` feature
  the day the final spec text ships. Additive: the single-revision `Registry`,
  `engine::validate`, and the report/golden artifacts are unchanged.
- **A dependency-floor honesty gate** (`cargo xtask minimal-versions`; scheduled
  `minimal-versions` CI job): the workspace's declared dependency floors
  (`Cargo.toml` `>=x.y.z`) are now the oldest versions it actually resolves to and
  builds/tests against, not assertions. `-Z direct-minimal-versions` pins every
  direct dependency to its floor, builds the whole workspace, and runs the engine
  suites there. Building this surfaced six floors sitting *below* the workspace's
  resolvable minimum — `serde` (→ 1.0.220), `serde_json` (→ 1.0.127), `tower`
  (→ 0.5.2), `tokio-util` (→ 0.7.9), and, in the host crate, `http` (→ 1.1) and
  `futures` (→ 0.3.30, whose old floor 0.3.0 is yanked) — each forced higher by the
  M2 server stack's transitive requirements and raised to the minimum the tree
  resolves to. Nightly-only (the flag is unstable), so a loud skip without it, and
  scheduled rather than per-PR since upstream churn can make a floor newly
  dishonest with no local change. Recorded as a new lens in the testing pyramid
  (`docs/plan/04-engineering-standards.md`).
- **A cross-architecture byte-identity check** (`cargo xtask cross-arch`;
  scheduled `cross-arch` CI matrix): the two engine crates (`mcp-conformance-core`,
  `mcp-trace-validator`) build and run their suites on every corner of the
  **(endianness × pointer-width)** square CI's own hosts leave untested, proving
  M1's "byte-identical reports across platforms" guarantee. Every CI host is
  64-bit little-endian (`x86-64`/`aarch64` Linux/macOS/Windows), so the canonical
  JSON form, the JSON/JUnit reports, and the golden corpus had only ever been
  pinned 64-bit little-endian. The three added corners (`cargo xtask cross-arch`:
  "3 architectures pass"):
  - `s390x` (64-bit **big-endian**) and `powerpc` (32-bit **big-endian**) under
    `qemu-user` — core 58 + validator lib 88 + golden 5 + readme 2 + pathological
    3, byte-identical, with the native frame-budget proof and the subprocess `cli`
    suite out of scope (an emulated stack / a cross-built child cannot exec
    without `binfmt`);
  - `i686` (32-bit **little-endian**) run **natively** via multilib — the *whole*
    suite, `cli` and the deep-stack proof included, byte-identical on 32-bit.

  Each arch runs on its own CI runner (the 32-bit `gcc-multilib` and the
  big-endian cross-gccs hard-conflict at the dpkg level); a target whose toolchain
  is absent skips loudly. Recorded as a new lens in the testing pyramid
  (`docs/plan/04-engineering-standards.md`).
- **A `changelog-links` gate** (`cargo xtask changelog-links`, in the per-PR
  `cargo xtask ci` set): every `## [X.Y.Z]` version heading in this file must
  carry a matching `[X.Y.Z]: <url>` reference definition, and `[Unreleased]:`
  must compare against the most recent released version. The sibling of
  `version-sync` for the other doc the release checklist forgets — see the
  Fixed entry below for the v0.3.0 defect that motivated it. `docs-links` could
  not catch it: that gate checks the definitions it *finds* resolve, not that a
  shortcut reference *has* one, and the `[Unreleased]:` target is an absolute
  URL it skips by design. RELEASING.md's prepare step now names the
  link-reference update.

### Changed

- **Dependabot no longer groups the rmcp SDK with routine dependency drift.** An rmcp
  bump is a deliberate upgrade requiring conformance re-validation (deferral
  `adopt-rmcp-enumnames-fix`), so bundling it with everything else made the whole PR
  unmergeable — [#28](https://github.com/tomtom215/mcp-conformance/pull/28) carried
  `rmcp 1.7 -> 2.2` (~70 compile errors) together with six harmless bumps, holding the six
  hostage to the one. `rmcp` and `rmcp-macros` now get their own group, so the SDK upgrade
  stays *visible* as milestone input while routine drift stays mergeable. The routine
  bumps #28 proposed are applied here directly (`serde_json` 1.0.151, `clap` 4.6.4,
  `tokio` 1.53.1, `http-body-util` 0.1.4, `futures` 0.3.33, `sse-stream` 0.2.5, and the
  transitive drift behind them); `rmcp` stays at 1.7.0, held there by the new
  `rmcp-macros` ceiling — which turns out to be a second job the shim does for free:
  rmcp 1.8.0 requires `rmcp-macros ^1.8.0`, so the bound keeps `cargo update` from
  silently performing the upgrade the deferral says must be deliberate. Why the pin
  holds rather than moving to `2.2.0` or the `3.0.0-beta` line that implements the next
  revision is now a recorded decision with a date and revisit conditions —
  [ADR-0011](docs/plan/decisions/0011-rmcp-pin-holds-at-1-7.md).
  The update moved the duplicate-version landscape, so `deny.toml`'s skip list moved
  with it: the `wit-bindgen` 0.51/0.57 split **collapsed** (its skip is retired, with a
  note saying why rather than a silent deletion — a skip that vanishes unexplained reads
  like a loosened policy), and `syn` 2/3 appeared in its place. That one is the ecosystem
  mid-migration: `serde_derive`, `clap_derive`, `async-trait`, `thiserror-impl` and
  `ref-cast-impl` have moved to syn 3 while seventeen other derive crates have not.
  Both are build-time-only proc-macro dependencies, so the cost is compile time rather
  than shipped surface, and holding our own derive crates back to force agreement would
  trade a real upgrade for a cosmetic graph. Skipped with the roots named and pinned
  exactly, so it re-fires the moment either version moves.

### Fixed

- **The scheduled dependency-floors gate, red since 2026-07-06** — a broken
  dependency pair upstream resolution actively prefers. `rmcp` declares its own
  proc-macro crate as `rmcp-macros = "^N.M.P"`, but the two are lockstep-coupled
  (the macro expands to calls into `rmcp`'s internals), so Cargo happily resolves
  `rmcp 1.7.0` with `rmcp-macros 1.8.0` — a pair that does not compile: 1.8.0's
  `#[tool]` expansion calls `rmcp::handler::server::common::schema_for_input`,
  which 1.7.0 does not export (added in 1.8.0), giving `E0425` at all five
  `#[tool]` sites in `mcp-everything-server`. The committed `Cargo.lock` hid it;
  the floors gate regenerates the lock, so it broke the first Monday after
  `rmcp-macros 1.8.0` was published (2026-06-23) and stayed broken for three
  scheduled runs. Repaired here with a documented **ceiling shim**
  (`rmcp-macros = ">=1.7.0, <1.8.0"`) beside the existing floor shims — it must
  move with the `rmcp` pin at the M2.5 upgrade, and fails loudly rather than
  silently if it does not. No runtime change: `rmcp-macros` was already in the
  tree as `rmcp`'s own dependency at exactly this version, and `Cargo.lock` gains
  only the new dependency edge. Reported as register row 3.13 and queued upstream
  as a one-line `=N.M.P` pin (engagement backlog item 11); the caret declaration
  is still present at `rmcp 3.0.0-beta.2`.
- **The CHANGELOG's link-reference definitions, stale since v0.3.0**: `[0.3.0]`
  had no `[0.3.0]: …` definition, so on GitHub it rendered as the literal text
  `[0.3.0]` instead of a release link; and `[Unreleased]` compared against
  `v0.2.0` rather than `v0.3.0`. Both corrected, and the new `changelog-links`
  gate above keeps the next release honest.

## [0.3.0] - 2026-06-14

> **Version-class call** (RELEASING.md: pre-1.0 minors may break, and the
> changelog says so explicitly): the next release is **0.3.0**, not 0.2.1.
> Two changes below are breaking — `TraceContext::new` (and therefore
> `engine::validate`) now panics on hand-built event slices whose `seq` is
> not strictly increasing, where 0.2.0 judged them silently wrong; and the
> newly judged TRAN-026 changes verdicts, so a trace containing a
> client-POSTed batch body that previously failed only generic message
> checks now also fails `transport.http-post-single-message`.

### Added

- **A project mdBook** (`book/`): a curated reader's guide — Introduction,
  Architecture, the trace format, the trace corpus, and conformance results —
  built and gated on every push by a `book` CI job (`mdbook build book`). The
  trace-format and corpus chapters embed the README's worked example and
  `corpus/README.md` verbatim via `{{#include}}`, so the book cannot drift from
  its sources; docs.rs completeness stays enforced by `missing_docs = "deny"`
  and the `--all-features` rustdoc gate. "Live" GitHub Pages deployment is the
  one owner-gated piece left.
- **Two more standing gates, and the release pipeline grows a third.**
  `cargo xtask version-sync` ties the README's stated crates.io version to
  `[workspace.package].version` (the README update the release checklist used
  to forget — the stale "0.1.0" a prior audit found, now a CI failure); a
  weekly `cargo-careful` job runs the engine crates' suites against a std with
  debug assertions and extra const-UB checks (a UB / integer-overflow
  regression a release build folds is now caught); and the release `verify`
  job runs `cargo xtask semver` (cargo-semver-checks vs the crates.io
  baseline), so an undeclared API break cannot ride a release alongside the
  declared behavioral ones.

- **Claims expire** (ADR-0010): three rounds of auditing found every
  falsehood in claims that were true once and never re-checked — so the
  repository now re-checks them itself. The deferral ledger
  (`docs/plan/deferrals.json`) gives every consciously deferred piece of
  work a review-by date; `cargo xtask deferrals --check` (weekly scheduled
  CI) fails once a row expires un-re-decided. First rows: the suite's
  `auth/*` client scenarios, the rmcp SSE-resumption upstream filing, the
  rust-sdk#902 offer clock, the register's 90-day sweep, and the suite
  0.2.0 pin bump. And the registry's verbatim quotes — verified in round
  two by a `/tmp` script that died with its session — are now re-verified
  weekly by `cargo xtask spec-drift` against the published spec text, under
  the normalization `SourceRef::quote` documents (italics/links/escapes
  unwrapped; list fragments verified verbatim per the `"; "` convention);
  first live run: 140/140 quotes verified. The registry's in-scope page set
  is finally explicit data (`registry/2025-11-25/sources.json`: the nine
  in-scope pages mapped to their published sources, plus every out-of-scope
  page of the revision with a verified reason — the gate keeps the list and
  the registry's citations identical in both directions).
- `cargo xtask ci` now runs the MSRV clippy leg CI runs (loud skip when the
  1.88 toolchain is absent), and `cargo xtask mutants` is the exact
  diff-scoped mutation gate from the PR workflow, computed against
  `origin/main` — the local-vs-CI gate skew that bit round two, mechanized
  away.
- Scheduled CI now accumulates evidence weekly instead of discarding it:
  grown fuzz corpora and criterion bench results upload as 90-day artifacts
  (the round-two "corpora seed-only" / "no bench history" deferrals,
  liquidated — benches/README.md records the posture). The tap's `loom`
  question is re-decided and recorded at the code it judges: nothing
  lock-free to model, uniqueness-only ordinal, real-parallelism stress test
  as the standing evidence.
- **The client gate is standing** (`cargo xtask conformance`, same CI job as
  the server leg): a child-process stdio smoke — the host binary spawning
  the everything-server binary over a real pipe, the one place two sibling
  executables can meet — then the four `2025-11-25` client scenarios run
  sequentially (client runs fail on WARNINGs and the `sse-retry` timing
  window is load-bearing, so parallel suite mode is deliberately not used),
  then the client-side agreement replay: every host-captured trace through
  `mcp-trace-validator` against `conformance/client-agreement-divergences.json`
  (same triage contract and both-directions staleness discipline as the
  server baseline; empty and live on first run — 4 sessions, zero
  unexplained divergence).
- **`mcp-reference-host`: the suite's client scenarios pass — all four, at
  the pin** (`initialize`; `tools_call` 1/1;
  `elicitation-sep1034-client-defaults` 5/5; `sse-retry` 3/3, inside the
  −50/+200 ms retry window with `Last-Event-ID` offered). What landed: the
  two real transports from rmcp's official client features (`proc` =
  child-process stdio, `http` = streamable HTTP over reqwest); the binary
  (`cli`) honoring the runner's contract (URL as final argument,
  `MCP_CONFORMANCE_SCENARIO` dispatch through the one `scenario.rs` table)
  with a hard `--deadline-secs` watchdog (the runner's 30 s kill reaches
  only its `sh -c` wrapper — an orphaned host would wedge the runner
  forever, measured); host-side trace capture (`capture`) as a `Transport`
  wrapper — redaction by construction, the message seam never sees headers
  — whose output is pinned against the validator's real reader and engine;
  and the spec's SSE-resumption dance (`resume`) on rmcp's public
  `StreamableHttpClient` seam, honoring the server-named `retry` through
  `RetryPolicy::delay_honoring_retry_after` (the load-bearing use ADR-0009
  predicted). rmcp 1.7's own transport cannot pass `sse-retry` — POST
  response streams reconnect-never and the in-flight call is lost; measured
  at source and on the wire (−53 ms "too early", no `Last-Event-ID`) —
  recorded as register row 3.12 and ADR-0009 §Amendment, upstream filing in
  the M4 backlog. `reqwest`/`futures`/`sse-stream` enter as direct
  dependencies of the `http` feature, version-mirroring rmcp's own tree.
- `mcp-everything-server`: `test_url_elicitation` — the URL-mode elicitation
  round trip (register 2.10 parity), closing the last interactive
  TypeScript-surface delta: a `mode: "url"` `elicitation/create` and, on
  consent, `notifications/elicitation/complete` for the issued id. The
  host↔server loop — consent recorded, id spent exactly once, by name;
  decline produces no completion — is pinned end to end in the host's
  `agent_loop` tests. The README's "needs a URL-capable client" deferral is
  closed, not restated.
- `mcp-trace-validator`: `transport.http-post-single-message` — TRAN-026
  ("The body of the POST request MUST be a single JSON-RPC request,
  notification, or response.") is now judged, with a killer trace
  (`tran-026-http-post-batch.jsonl`). Its previous exclusion claimed a
  multi-message body "cannot be represented in a trace" — untrue (the payload
  is an arbitrary JSON value, and a batch was only caught generically under
  BASE-008, never attributed to TRAN-026). Registry: 140 entries, 51 judged
  by 47 checks, 89 documented exclusions.
- Registry `TRAN-049`: the transports page states the client POST obligation
  twice (an intro sentence and a numbered step three lines apart); only one
  sentence was an entry. "Every MUST on an in-scope page enters — no
  exceptions" now holds for both, the restatement excluded with prose naming
  its twin.
- `mcp-everything-server`: two tests the registry's exclusions claimed
  existed but did not — `unsupported_protocol_version_is_rejected_with_400`
  (TRAN-020: pins rmcp 1.7's in-session 400; the initialize exchange itself
  never consults the header, measured) and `default_bind_is_loopback`
  (TRAN-008: every other test passes `--bind` explicitly and would never
  notice a widened default).
- **`mcp-reference-host`: the host exists** (M3 opens; ADR-0009). Three
  transport-agnostic pieces, tested in-process against the real
  `mcp-everything-server`: `script` (every model/user behavior as data —
  sampling reply, SEP-1034-defaults/fixed/decline/cancel elicitation
  policies, URL-mode consent, roots; zero model-provider network use by
  construction), `handler` (the `rmcp::ClientHandler` answering from a
  script, with an event log and a pending-id set enforcing the URL-mode
  client MUST — unknown or already-completed `elicitationId` completions
  are observably ignored), and `run` (the bounded loop: scripted calls or
  discover-and-call-once with schema-derived arguments — local `$ref`s
  resolved, enum-as-`oneOf`/`const` sampled — under the stop-condition
  lattice cancellation > turn limit > error budget > completion, every
  variant a tested stop reason, in-band `isError` results counting against
  the budget like protocol errors). The SEP-1034 path round-trips against
  the same `test_elicitation_sep1034_defaults` tool the server-side suite
  run exercises, with the wire content pinned byte-for-byte. The client-SUT
  contract was decoded from the pinned suite 0.1.16 bundle (URL appended as
  the command's final argument, `MCP_CONFORMANCE_SCENARIO`/`_CONTEXT` env,
  30 s budget; four protocol scenarios + fourteen deferred `auth/*` ones)
  and recorded in ADR-0009. Binary, transports, and suite wiring are the
  next slice; the crate README states exactly what is and is not here.
- `mcp-everything-server`: `get-structured-content` — the TypeScript
  everything server's structured-output tool, mirrored exactly (the zod city
  enum, the weather fixtures, derived `outputSchema`, `structuredContent`
  plus the backward-compatible JSON text block). The M2 line claimed "parity
  with the TypeScript everything server's surface" while the server had no
  `outputSchema` tool at all — the suite never exercises one, so nothing
  noticed. The roundtrip test pins the TOOL-010/TOOL-011 pairing the spec
  requires of any server declaring an output schema. The two remaining
  TypeScript-surface deltas (URL-mode elicitation, async sampling) are now
  documented decisions with reasons in the crate README, not silences.
- `mcp-trace-validator`: pathological-input boundedness tests — 100k-event
  sessions validate with correct verdicts, 20k-fold request-id reuse stays
  linear and is flagged, and hostile deep nesting is rejected at parse with
  the offending line named (never a stack overflow, never judged anyway).
  benches/README.md records the re-affirmed no-timing-gate decision: still
  no measurement history, but complexity is now gated by these tests.
- Concurrency and crash-durability proofs for the session tap, replacing
  reasoning with evidence: 16 sessions recording through one writer at real
  parallelism (per-file `seq` contiguous from 0, every file parses through
  the real reader, zero cross-session bleed), and a SIGKILL mid-burst
  integration test pinning the documented durability shape — every persisted
  line parses, at most the final line may be torn.
- `TraceContext::new` (and so `engine::validate`) now *enforces* the
  strictly-increasing-`seq` contract with a documented panic instead of
  judging a contract-violating hand-built slice silently wrong; the
  session-id mutants exclusion's "one event owns one seq" justification now
  names this enforcement rather than assuming the reader is the only path.

### Changed

- Third-audit census closures: the readiness line (`listening on `) is
  single-sourced as `mcp_everything_server::READINESS_LINE_PREFIX` — the
  cross-process contract orchestration waits on — with the binary tests
  pinning the literal independently and xtask's copy carrying the pointer;
  the corpus README states the violation-trace naming contract the golden
  harness enforces (`area-nnn-…` must falsify `AREA-NNN` by name); the
  pathological-input tests document their honest limit (a quadratic-but-
  correct mutant passes unless it blows the mutation timeout — verdicts and
  hangs are the caught classes, by design); and the core README's "every
  in-scope normative clause" claim now names its universe
  (`sources.json` + the spec-drift gate) instead of leaving "in-scope" to
  judgment.
- Two gates can no longer be fooled the way this audit's own tooling was:
  `docs-links` now also checks reference-style definitions (`[label]:
  target` — previously the gate's one false-negative path; today's are all
  external, but a relative one would have passed unchecked), and
  `file-sizes` fails when its scan finds implausibly few files instead of
  reporting a vacuous green over an empty walk.
- `mcp-everything-server`: session-id entropy is pinned, not assumed —
  `session_ids_are_version_4_uuids_and_distinct` asserts the v4-UUID
  version/variant nibbles and distinctness on real initialize responses,
  and TRAN-010's exclusion now cites it (TRAN-011's visible-ASCII check
  would never notice a regression to sequential ids).
- The tap tells the truth about its failure modes, loudly: a non-UTF-8 SSE
  chunk now stops recording that stream (the doc always said "abort"; the
  code cleared the buffer and kept parsing — resuming after a dropped chunk
  can mis-frame everything that follows), and a non-empty request body that
  is not JSON is reported to stderr instead of leaving a silent hole a trace
  reader would misread as "no body". Module docs now state the real
  durability contract: flushed records survive a kill, queued records die
  with the process, the final line may tear.

### Fixed

- **A fuzz harness that contradicted its own unit test** (third audit, found by the
  first real CI run of the weekly fuzz job — dispatched precisely because a
  never-run gate is not a gate). The `canonical_json` fuzz target asserted
  `parse(canonical(v)) == v` over `serde_json::Value` and called it "round-trip
  exact" — but canonicalization deliberately folds representations (RFC 8785 maps
  `-0.0` → `0`, `2.0` → `2`), so that claim is false by design, and the
  `canonical_form_is_a_parse_fixpoint` unit test had always (correctly) asserted
  the *idempotence* property instead. The two disagreed; only the fuzzer, on its
  first generated `-0.0`, could expose it. The canonicalizer was always correct
  (its `-0.0 → 0` fold is RFC 8785 Appendix B, already unit-tested). Fixed: the
  fuzz target now asserts the same idempotence
  (`canonical(parse(canonical(v))) == canonical(v)`); the crashing input is pinned
  as the corpus seed `seed-negative-zero-fold` and as a `cargo test` regression
  (`negative_zero_fold_is_idempotent_not_representation_preserving`); and all three
  fuzz targets were re-run clean (canonical_json 3.5M execs, registry_parse 3.9M,
  trace_parse 12.8M). The census this round was scoped to read `fuzz_targets/*.rs`
  and missed the contradiction — recorded so the next round's census cross-checks
  paired tests of one function, not each in isolation.
- The round's closing verification ran as its floor and its new dimension:
  the full `--all-features` mutation sweep — now **857 mutants** (the round
  added ~109 mutable sites): 741 caught, 116 unviable, **0 missed**, 42
  minutes — and, for the first time, **miri over `mcp-conformance-core`**
  (63 tests, 0 findings; isolation disabled for proptest's cwd persistence,
  and the 50k-deep canonicalization proof runs at depth 500 under
  `cfg(miri)` — the interpreter checks the walker for UB there, not for
  native frame budget, which stays a native-only proof). `cargo audit`:
  233 dependencies, no advisories. `cargo package --workspace --exclude
  xtask --locked`: green. Both conformance legs re-confirmed on the final
  tree: server 40/40 with 30-session agreement, client smoke + 4 scenarios
  with 4-session agreement — zero unexplained divergence everywhere.
- `conformance/expected-failures.yaml` used a `failures:` key the pinned
  runner has never read: the 0.1.16 loader consumes exactly `server:` and
  `client:` keys and silently ignores everything else, so the committed
  baseline was a no-op that happened to coincide with reality (zero expected
  failures). The file now uses the real schema, documents the silent-ignore
  hazard, and carries the (empty) `client:` section the client gate reads.
- The full `--all-features` mutation sweep (748 mutants, 31 minutes) ran as
  the audit's closing verification: 641 caught, 105 unviable, 0 timeouts,
  and exactly 2 missed — both in the tap's non-JSON-body note, code this
  same audit had added hours earlier (its guard had no observer). The note
  is now session-scoped (it can never claim a recording that did not
  happen) and counted against the real binary's stderr; both mutants were
  re-applied by hand and die against the counting test.
- Error-path tests now pin *which* error, not just that one occurred —
  six sites asserted only `is_err()`, and one of them proved able to hide a
  deleted security gate: with the sampling capability gate removed, the old
  assertion stayed green (the doomed `sampling/createMessage` failed
  downstream as `-32603` — after an illegal request had already gone out on
  the wire) while the strengthened test fails (demonstrated by neutering the
  gate). Pinned: the gate's `-32600` and message, resource-not-found's
  `-32002` (the repo's only deliberate use of it), and `-32602` at the four
  parameter-boundary sites whose comments claimed the code the assertions
  never checked.
- The golden corpus now enforces attribution by name: a violation trace
  `area-nnn-…` must produce a Fail/Warn row with findings for exactly
  requirement `AREA-NNN` — previously a defect re-routed to the wrong
  requirement could re-bless silently, guarded only by global check-ID
  set-equality and human diff review. Also: every golden must belong to a
  living trace (orphan sweep), and blessing requires `BLESS=1` exactly,
  matching the coverage manifest's convention (`BLESS=0` no longer blesses).
- The tap's every-platform validator round-trip now fails on *any*
  MUST-level finding, not only `LIFE-*` — a tap serialization regression
  that manufactures transport or base findings (wrong header recording,
  broken `seq`, mangled payloads: precisely what the tap exists to get
  right) was previously visible only in the npx-gated conformance job.
- Five registry exclusions said things the code disproves, found by tracing
  every "enforced instead" pointer to its target: TOOL-012 cited "policy
  tests" for four duties of which two (rate limiting, output sanitization)
  are implemented nowhere; RES-005 cited the wrong test file; TRAN-003
  claimed non-UTF-8 bytes "surface as capture-time read failures" while the
  tap silently skips them; TRAN-008 named the wrong enforcement site;
  LOG-002 called heuristic verdicts "non-deterministic" when the defect is
  unsoundness. Each now states what is actually enforced, where, and by
  which named test.

- The trusted-publishing record was false everywhere it appeared: RELEASING.md,
  ADR-0007's amendment, `release.yml`'s comments and run summary, and the
  v0.2.0 changelog entry all asserted "Trusted Publishing Only" enforced on all
  four crates as of 2026-06-10 — disproven by the v0.2.0 publish itself, whose
  first attempt failed with crates.io's `400: No Trusted Publishing config
  found for repository tomtom215/mcp-conformance`. Every site now states
  exactly what the evidence supports: the config was added 2026-06-11 and is
  proven by the OIDC publish of all four crates; the "Trusted Publishing Only"
  toggle, the bootstrap secret's deletion, and the token's revocation are
  owner-visible only — the owner confirmed on 2026-06-11, after the
  correction landed, that trusted publishing is working as intended
  (ADR-0007 §Correction records the confirmation and its weight).

## [0.2.0] - 2026-06-11

### Added

- **Registry completeness audit (2026-06-11)**: clause-by-clause re-extraction
  of the `2025-11-25` spec found 68 in-scope normative clauses missing from
  the registry; all are now entries (71 → 139), every quote verified verbatim
  against the published text. Four are mechanically checkable and gained
  checks plus killer traces: `lifecycle.initialize-result-shape` (LIFE-010 —
  the initialize result must carry `capabilities` and `serverInfo`),
  `transport.client-accept-header` (TRAN-025/TRAN-039 — every client request
  must list `text/event-stream` in `Accept`), `transport.success-content-type`
  (TRAN-029/TRAN-040 — HTTP 200s must answer `application/json` or
  `text/event-stream`), and `base.meta-key-format` (BASE-019/BASE-020 — the
  `_meta` key prefix/name grammar, scoped to the `params`/`result` envelope
  positions where user data cannot collide). The other 61 carry documented
  exclusions naming exactly why a recorded trace cannot judge them (stream
  identity, request methods, timing, and server-internal ground truth are
  not in the capture vocabulary). The agreement check over the suite's 30
  tapped sessions runs the new checks at zero unexplained divergence.

- **The agreement check is live** (docs/plan/03-conformance-strategy.md
  §Calibration): `mcp-everything-server` gains a session trace tap (feature
  `tap`, `--tap-dir`, HTTP transport) recording every admitted suite session
  as a validator-ready JSON Lines trace — allowlisted headers only, so
  credential-bearing headers are never captured; the writer assigns `seq`
  per file so the schema's strictly-increasing rule holds even when POST
  exchanges and SSE streams record concurrently. `cargo xtask conformance`
  now replays every tapped session through `mcp-trace-validator` and fails
  on any MUST-level finding not explained in
  `conformance/agreement-divergences.json` (triage class `our-bug` |
  `suite-bug` | `spec-ambiguity` plus an upstream link required; unknown
  fields rejected), writing the full reconciliation to
  `target/conformance/agreement.json`. First run: 30 sessions, zero
  unexplained divergence — and one real catch each way: a MUST divergence
  triaged suite-bug (#7: the runner's dns-rebinding client skips
  `notifications/initialized`) and an informational SHOULD warning on the
  suite's deliberate version-compat probe (TRAN-018).
- **Coverage manifest** (`conformance/coverage-manifest.json`): generated
  from the tapped sessions and checked on every conformance run (`BLESS=1`
  regenerates) — the server's declared capabilities, all eight server-party
  registry capability gates (each must be declared: the gate caught the
  missing `listChanged` declarations on first run), and the 18 wire methods
  the suite exercises.
- `mcp-everything-server`: `test-list-changed` tool emits the three
  `notifications/*/list_changed` messages, and the server now declares
  `listChanged` for tools, resources, and prompts — declared because
  exercisable, per the capability-honesty rule.
- `mcp-conformance-core`: `TraceEvent::new` — the constructor capture
  tooling needs (`TraceEvent` is `#[non_exhaustive]`, so out-of-crate
  literals don't compile).
- `mcp-everything-server`: `tap::RECORDED_HEADERS` is now public — the
  recording allowlist is worth inspecting, and the doc gate (now run with
  `--all-features`) caught a private-intra-doc link that made it so.
- `mcp-everything-server`: streamable HTTP serving (`--transport http`)
  behind the default-secure `Host`/`Origin` policy — 403 before any MCP
  processing, loopback-only by default, `--allowed-host` /
  `--dangerously-allow-any-host` to widen. The full official-suite server
  surface is implemented (suite-defined `test_*` tools incl. sampling and
  the three elicitation scenarios, resources + template + subscriptions,
  four prompts, completion, logging level filtering): **100% pass on
  @modelcontextprotocol/conformance 0.1.16's active `2025-11-25` server
  scenarios** (40 checks), verified against the real runner.
- `mcp-everything-server`: the M2 build-out begins on rmcp 1.7 — the
  `EverythingServer` handler (protocol `2025-11-25`, capabilities advertised
  only once implemented), the tool module (`echo`, `add`, TypeScript
  everything-server phrasing), and a stdio binary
  (`mcp-everything-server --transport stdio`). In-process duplex round-trip
  tests drive a real rmcp client against the server with no sockets.

### Changed

- **MSRV raised from 1.85 to 1.88** — rmcp's measured compilation floor
  (let-chains in its library source; undeclared upstream). Per policy
  (ADR-0004/ADR-0008) this makes the next release **0.2.0**.
  *(Corrected 2026-06-27: the floor is **not** let-chains in rmcp's own source —
  a later empirical re-test (2026-06-15; toolchains 1.85.0/1.87.0/1.88.0,
  `--locked`; no `&&`-joined let-chains in `crates/rmcp{,-macros}/src`) disproved
  that — but the transitive `darling 0.23.0` (`rust-version = 1.88.0`), pulled in
  by `rmcp-macros`, which cargo enforces with a clear named pre-check rather than
  an opaque `E0658`. Our own workspace independently uses let-chains, an additional
  reason for the floor. See ADR-0008 §Correction and register 3.5. The v0.2.0
  GitHub Release body carries the original wording.)*
- Release pipeline is OIDC-only: the one-time bootstrap conditional is removed
  from `release.yml`; the publish job authenticates exclusively via trusted
  publishing (ADR-0007). *(Corrected 2026-06-11: this entry originally asserted
  "Trusted Publishing Only" enforcement and token revocation as fact — neither
  was verifiable from this repository, and the enforcement claim was false when
  written; see ADR-0007 §Correction. The v0.2.0 GitHub Release body carries the
  original wording.)*

### Fixed

- `mcp-conformance-core`: `to_canonical_string` walks nesting with an explicit
  heap work-stack instead of recursion — a deeply nested hostile value can no
  longer overflow the call stack (an uncatchable abort). Output is
  byte-identical.
- `mcp-conformance-core`: `EventBody::Http` normalizes header field names to
  lowercase on deserialization (HTTP names are case-insensitive, RFC 9110
  §5.1). Previously a trace recording on-the-wire casing (`Mcp-Session-Id`,
  `Mcp-Protocol-Version`) slipped past the case-sensitive transport checks,
  hiding a bad session id or protocol version behind its capitalization.
- `mcp-trace-validator`: BASE-004/BASE-009 now flag a request answered by
  *both* a result and an error (each check formerly tracked only its own
  response flavor and saw a clean one-to-one).
- `mcp-trace-validator`: JUnit XML escaping substitutes the C0 control
  characters XML 1.0 forbids entirely (other than tab/LF/CR), so a report can
  never be an ill-formed document a strict CI parser rejects.
- `mcp-everything-server`: the session tap's SSE splitter now stops
  recording a stream whose un-delimited frame outgrows the recording budget
  (the same 4 MiB bound the JSON path already had) instead of buffering it
  without limit — recording is diagnostics and must never be what takes the
  server down. The stream itself still flows to the client untouched.
- `mcp-everything-server`: the tap records repeated HTTP header field lines as
  their comma-joined value (RFC 9110 §5.3), so a split `Accept` header is
  captured faithfully rather than truncated to its first line.
- Release packaging excludes `xtask` (`publish = false`, but
  `cargo package --workspace` still packaged it; v0.1.0's GitHub Release
  carries the stray — harmless — crate file).

### Security

- `mcp-everything-server`: the `Host`/`Origin` 403 gate now fails closed on
  duplicate `Host` or `Origin` headers (a smuggling shape — it previously
  judged only the first value while a downstream consumer could key off a
  later one). A well-formed request carries exactly one of each.
- `mcp-everything-server`: the per-session `resources/subscribe` set is
  capped, so a hostile client cannot grow its bookkeeping without bound.

## [0.1.0] - 2026-06-10

First release: the `2025-11-25` requirement registry and the offline trace
validator, at the gates documented in [docs/plan/04-engineering-standards.md](docs/plan/04-engineering-standards.md).

### Added

- `mcp-conformance-core`: requirement registry model (RFC 2119 levels, verbatim
  spec quotes, SEP-2484-shaped check-or-exclusion traceability, ADR-0006
  capability gates) covering the `2025-11-25` core protocol surface — base
  protocol, lifecycle, transport security, tools, resources, prompts, logging,
  completion, and pagination, stored as per-area registry files; JSON Lines trace
  event schema; JSON-RPC message classification; canonical JSON serialization with
  RFC 8785 object-key ordering and ECMAScript number formatting validated against
  the RFC's Appendix B vectors.
- `mcp-trace-validator`: deterministic validation engine spanning every registry
  area, with request/response exchange pairing and not-applicable accounting for
  capability-gated requirements; human/JSON reports with
  pass/fail/warn/excluded/unsupported/not-applicable accounting; CLI with
  documented exit codes (0 pass, 1 findings, 2 invocation problem, 3 malformed
  trace); golden-corpus test harness with falsifiability enforcement (every check
  killed by a committed violation trace) and a provenance-ledger invariant;
  criterion benchmarks (unmonitored by CI — see `benches/README.md`).
- `mcp-everything-server`: default-secure HTTP transport policy — loopback-only
  `Host`/`Origin` allowlisting, fail-closed parsing, explicit
  `dangerously_allow_any_host` opt-out.
- `cargo xtask coverage`: generates the README's per-area requirement-coverage
  table from the registry; CI verifies it never drifts.
- CI: informational `-Zminimal-versions` job proving the workspace dependency
  floors build and pass tests.
- Release pipeline (`release.yml`, ADR-0007): tag-triggered, rehearsable via
  `workflow_dispatch`; full gates + cross-OS tests, SLSA build-provenance
  attestation with a byte-identity check between attested and published
  packages, idempotent GitHub Releases, resumable dependency-order publishing,
  and OIDC trusted publishing after a one-time bootstrapped token release.
- `mcp-reference-host`: deterministic retry/backoff policy with caller-supplied
  jitter and capped `Retry-After` honoring.
- Workspace tooling: `cargo xtask ci` (all local gates) and `cargo xtask bless`
  (golden regeneration); CI with format/clippy/test matrices (stable + MSRV 1.85 ×
  Linux/macOS/Windows × three feature modes), docs, `cargo-deny`, package
  validation, diff-scoped mutation gate on PRs, and scheduled RustSec audit + full
  mutation sweep.

[Unreleased]: https://github.com/tomtom215/mcp-conformance/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/tomtom215/mcp-conformance/releases/tag/v0.4.0
[0.3.0]: https://github.com/tomtom215/mcp-conformance/releases/tag/v0.3.0
[0.2.0]: https://github.com/tomtom215/mcp-conformance/releases/tag/v0.2.0
[0.1.0]: https://github.com/tomtom215/mcp-conformance/releases/tag/v0.1.0
