<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0012: Subject Counting and the `not-observed` Outcome

**Date:** 2026-08-17
**Status:** Accepted
**Author:** Tom F.

---

## Context

[ADR-0006](0006-capability-gated-applicability.md) fixed one half of an
argument and left the other half open. Its reasoning was that a requirement
gated on a capability the session never negotiated must not be reported as
*passed*, because "vacuous passes inflate scores and lose trust". That
reasoning is not about capabilities. It is about evidence, and it applies
wherever a check has nothing to look at.

The engine did not apply it anywhere else. A check that ran, iterated a trace,
matched nothing, and returned an empty finding list was classified exactly like
a check that examined real traffic and found it conforming — both produced
`pass`. The consequences were not marginal:

- A trace consisting of a four-message handshake reported 38 passes against the
  `2025-11-25` registry. It had complied with almost nothing; it had merely
  never been asked.
- The three committed `2026-07-28` captures reported 123–124 passes apiece.
  Their rows were near-identical, which read as three independent
  confirmations. In fact the stdio capture was the only one that opened a
  subscription or ran an MRTR round, and the two HTTP captures were the only
  ones that touched prompts or resource templates. The report could not say so,
  and `corpus/README.md` had recorded that indistinguishability as a known
  limitation of the format.
- A regression that stopped a check from *reaching* its subject — a filter
  narrowed too far, a payload path renamed — looked identical to the check
  passing. Several such defects had already been found by hand (`TRAN-071`,
  the five `2026-07-28` capability checks, `BASE-032`/`BASE-036`); nothing in
  the report would have surfaced them.

A conformance verdict's whole value is that it distinguishes *complied with*
from *did not comply*. A report that cannot also distinguish *never came up*
is not reporting conformance; it is counting checks.

## Decision

### Checks count their subjects

Every check reports how many **subjects** it considered, through
`FindingSink::examined`. The counting rule is written once, on that method,
and applied identically by every check:

> A subject is a trace element the check *considered* — one that, with
> different content, could have produced a finding. Count it after the filters
> that define the clause's scope, and before the condition that makes an
> element a violation.

The rule's two halves are both load-bearing. Counting before the scope filters
would make every clause observed by any trace with a message in it. Counting
after the violation condition would count only findings, which is the number
the report already has.

### Prohibitions come in two shapes

Where a clause forbids an element from having a property — "notifications MUST
NOT include an ID" — the element is the subject, and a session with no
notifications has not tested it.

Where a clause forbids an element from *existing at all* inside a window — "the
server SHOULD NOT send requests before `initialized`" — the **window** is the
subject, because sending nothing through a window the trace actually shows is
precisely what compliance looks like there.

Getting this wrong is not theoretical: `LIFE-004` and `LIFE-005` state the same
rule about the same window from the two sides, and counting the forbidden
element reported them differently — `pass` and `not-observed` — on every trace
where both held, purely because the client always sends `initialize` and a
well-behaved server sends nothing.

### The outcome

A requirement whose checks found **no subject** and reported **no finding** is
`not-observed`: a first-class outcome in `Totals`, beside `excluded`,
`unsupported` and `not-applicable`. It renders as `NOBS` in the human report
with a reason line, `not-observed` in JSON and in the multi-revision table, and
`<skipped>` in JUnit with its own message. The verdict and exit codes ignore it,
exactly as they ignore `excluded` and `not-applicable`.

A requirement backed by several checks is observed if **any** of them found a
subject. Sharing a check across clauses that state one rule in several sections
is deliberate (see `checks/mod.rs`); a clause must not report unobserved because
one of its several checks happened to abstain.

Precedence is unchanged from ADR-0006 and this outcome sits at the end of it: a
documented exclusion wins, then a registry/build check mismatch, then the
capability gate, then the checks run — and only then can subject count decide
between `pass` and `not-observed`. `not-applicable` therefore continues to
answer "the clause does not bind this session"; `not-observed` answers the
different question "it binds, and the session never exercised it".

### The invariant that keeps it honest

`checks_count_their_subjects` (in `tests/golden.rs`) asserts that every
registered check examines at least one subject on at least one committed trace.
It is the counterpart of `corpus_falsifies_every_check`: that one proves a check
*can fail*, this one proves its *pass can mean something*. Without it, a check
that forgets to count would report every clause it backs as `not-observed`
forever, silently — the same class of defect in the opposite direction.

## Consequences

### Positive

- Pass counts become measurements. The conforming `2025-11-25` sessions report
  16–35 passes where they reported 38–52, and the `2026-07-28` captures 56–59
  where they reported 124. No verdict moved: across all 130 golden reports the
  only outcome transition is `pass` → `not-observed`.
- The corpus's captures now differentiate each other. `SUBS-001`/`002`/`005`/
  `006` and `BASE-039` are evidenced by the stdio capture alone and read
  `not-observed` on the two HTTP ones; `PROM-016` and `RES-018` go the other
  way. The limitation `corpus/README.md` recorded is closed.
- A check that stops reaching its subject now moves a golden. The failure mode
  that produced `TRAN-071` and the five `2026-07-28` capability defects becomes
  visible in a diff instead of requiring someone to notice.
- The report states an honest denominator for coverage claims: "59 of the 124
  judgeable clauses were exercised by this session" is a sentence the tool can
  now support.

### Negative

- Every new check must count, and the rule takes judgement to apply — the
  scope/violation boundary is a reading of the clause, not a mechanical
  property of the code. The invariant test catches a check that never counts;
  it cannot catch one that counts in the wrong place. Each check's placement is
  therefore commented where it is not obvious.
- Headline numbers dropped by roughly half, and the earlier figures are
  published (crates.io, the book, the README). They are corrected in place with
  the reason stated rather than quietly restated, because the numbers were
  wrong and a conformance tool that silently revises its own scores has a worse
  problem than a large correction.
- `subjects` is part of `CheckOutcome`'s public shape. It is a plain `u32`
  rather than `Option<u32>`: an "uninstrumented" sentinel would be
  indistinguishable from "examined nothing", and a fail-safe that treats
  unknown as observed reinstates exactly the vacuous pass this ADR removes.

## Alternatives Considered

### Leave `pass` alone and add a separate "coverage" report

Rejected. It puts the honest number in a second artifact that consumers of the
verdict — CI, SEP-1730 tier evidence, a reader of the README — will not see.
The defect is in what `pass` claims, and it has to be fixed there.

### Infer observation from the trace instead of from the checks

Tempting, because it needs no per-check work: decide from the session's traffic
which areas were exercised, and mark the rest unobserved. Rejected — it is a
second, parallel model of what each clause binds to, and the moment it
disagrees with the check, the report is wrong in a way nothing tests. The check
already knows what it looked at; asking it is the only answer that cannot drift.

### `Option<u32>` with `None` meaning "not instrumented"

Held during the migration and then removed. While checks were being converted
it distinguished "not yet counting" from "counted nothing", and the engine
treated `None` as observed to preserve behaviour. Once every check counted, the
distinction was unrepresentable — a check examining nothing leaves the counter
at its initial value either way — and keeping the `Option` would have meant a
sentinel that lies in the direction of the original defect.
