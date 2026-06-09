<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Architecture Decision Records

**Status:** Active
**Last reviewed:** 2026-06-09

---

Append-only log of decisions with consequences. The format and discipline follow
[a2a-rust's ADR practice](https://github.com/tomtom215/a2a-rust/tree/main/docs/adr): a
decision is recorded when it constrains future work, when reversing it would be expensive,
or when the reasoning would otherwise live only in someone's head.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-plan-documentation-model.md) | Plan Documentation Model | Accepted |
| [0002](0002-product-scope.md) | Product Scope: Conformance Toolkit, Not Another SDK | Accepted |
| [0003](0003-crate-naming.md) | Crate Naming and Namespace Strategy | Accepted |

## Process

1. **Numbering** is sequential, four digits, never reused.
2. **Statuses:** `Proposed` → `Accepted` | `Rejected`; later `Superseded by ADR-NNNN`.
   A superseded ADR is never edited beyond its status line — the old reasoning stays
   readable.
3. **When required:** any architectural decision made or revised in a PR ships an ADR in the
   same PR (the a2a-rust checklist rule). Scope changes against the
   [charter](../00-charter.md) always require one ([risk R7](../08-risk-register.md)).
4. **Size:** one decision per ADR. Linked decisions get linked records.

## Template

```markdown
<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR NNNN: Title

**Date:** YYYY-MM-DD
**Status:** Proposed | Accepted | Rejected | Superseded by ADR-NNNN
**Author:** Name

---

## Context

What is true, what forces apply, what problem demands a decision. Facts cite the
[ecosystem register](../01-ecosystem-context.md) or primary sources.

## Decision

What we will do, stated so compliance is checkable.

## Consequences

### Positive

### Negative

Real costs, honestly. An ADR with no negative consequences hasn't found them yet.

## Alternatives Considered

Each with the reason it lost. "Rejected because" sentences, not strawmen.
```
