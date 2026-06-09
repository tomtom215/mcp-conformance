<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Project Governance

## Roles

### Contributor

Anyone who opens a pull request, files an issue, or participates in discussions. No
special permissions required.

### Committer

Trusted contributors with write access: may merge pull requests and triage issues.
Nominated by maintainers on the basis of sustained, high-quality contributions —
including corpus and documentation work, which count fully.

### Maintainer

Maintainers set technical direction, approve architectural changes (every one is
recorded as an ADR in [docs/plan/decisions/](docs/plan/decisions/README.md)), manage
releases, and administer the repository. Final decision authority rests here.

**Current maintainers**

| Name   | GitHub                                     | Role       |
| ------ | ------------------------------------------ | ---------- |
| Tom F. | [@tomtom215](https://github.com/tomtom215) | Maintainer |

## How decisions are made

1. **Reversible, in-scope changes**: normal PR review.
2. **Architectural decisions**: ADR in the same PR, reviewed together.
3. **Scope changes** (anything touching the charter's goals or non-goals): ADR plus an
   explicit charter amendment — see risk R7 in
   [docs/plan/08-risk-register.md](docs/plan/08-risk-register.md). The non-goals are
   load-bearing; they change deliberately or not at all.
4. **Conformance verdict policy** (what counts as pass/fail/excluded): treated as
   architectural — the accounting rules in
   [docs/plan/03-conformance-strategy.md](docs/plan/03-conformance-strategy.md) are
   the project's trust contract.

## Relationship to upstream

This project operates under the Model Context Protocol's governance for anything
spec-adjacent (SEP process, official-suite authority) and never positions itself as
speaking for the MCP project. See
[docs/plan/07-ecosystem-engagement.md](docs/plan/07-ecosystem-engagement.md).

## Conduct

The [Code of Conduct](CODE_OF_CONDUCT.md) applies in all project spaces and is
enforced by the maintainers.
