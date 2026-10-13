<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Contributing

Thank you for considering a contribution. This project holds every line — code, config,
documentation — to the standards in
[docs/plan/04-engineering-standards.md](docs/plan/04-engineering-standards.md). This
file is the practical version of that document.

## Before you build anything substantial

This project is **upstream-first**: anything generically useful to the official MCP
Rust SDK (`modelcontextprotocol/rust-sdk`) or the official conformance suite
(`modelcontextprotocol/conformance`) should be proposed there before it lands here.
Read [docs/plan/07-ecosystem-engagement.md](docs/plan/07-ecosystem-engagement.md), then
open an issue describing your approach before writing a large PR.

## Setup

Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pins the exact stable
toolchain the gates run on, and rustup installs it on the first `cargo` command in
the checkout, so your lints are CI's lints. The crates' MSRV is 1.88; CI also runs
clippy on it, so code must compile on both.

```bash
git clone https://github.com/tomtom215/mcp-conformance
cd mcp-conformance
cargo xtask ci    # run every local quality gate, in CI order
```

## Quality gates

All of these must pass before merging — `cargo xtask ci` runs them in order:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings` — also with
   `--no-default-features` and `--all-features`
3. `cargo test --workspace` — also with `--no-default-features` and `--all-features`
4. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` — also with
   `--all-features` (feature-gated modules carry rustdoc too)
5. `cargo clippy` on the MSRV toolchain — skipped **loudly** when clippy for
   that toolchain is not installed (CI runs it regardless)
6. `cargo deny --all-features check` when cargo-deny is installed — skipped
   **loudly** otherwise (`cargo install cargo-deny --locked` to run it locally;
   CI runs it regardless, so a dependency-policy violation will fail there)
7. The 500-line cap on source and registry files (`cargo xtask file-sizes`)
8. Every fuzz target source declared as a `[[bin]]`, so `cargo fuzz list` —
   which the weekly fuzz job takes its target list from — can see it
   (`cargo xtask fuzz-targets`)
9. Every relative documentation link resolving, and every book chapter reachable
   from `SUMMARY.md` — mdBook renders only what the summary lists and says
   nothing about the rest (`cargo xtask docs-links`)
10. The README's crates.io version tracking the workspace version
    (`cargo xtask version-sync`), and every `CHANGELOG` heading carrying its
    reference definition (`cargo xtask changelog-links`)
11. The README coverage table in sync with the registry (`cargo xtask coverage --check`)
12. `corpus/README.md`'s per-capture coverage table in sync with the committed
    golden reports, and every count stated in the **living** Markdown agreeing
    with the committed artifacts — the "N of the M judgeable clauses" claims and
    the "N pass, M fail" verdicts against the golden reports, and the "N passing
    / M failing" readiness scores against `conformance/draft-readiness.json`
    (`cargo xtask draft-coverage --check`). Living means the
    documents a reader treats as current: the READMEs, `CONTRIBUTING`, the
    `Unreleased` changelog, `docs/plan/*.md` and the book. Dated documents are
    exempt and stay as written — `docs/reports/`, the ADRs under
    `docs/plan/decisions/`, and released changelog sections. To quote a number
    you are *not* asserting — a line as it used to read, or sample tool output
    — put it in backticks; code is a specimen, prose is a claim
13. The book's cross-SDK matrix (`book/src/sdk-matrix.md`) in sync with the
    committed golden reports of the official SDKs' conformance servers, and the
    book's worked guide still including, from `interop/matrix.sh`, the commands
    the scheduled `sdk-matrix` job runs (`cargo xtask sdk-matrix --check`)

Gates 7–13, plus seven structural checks — SPDX headers, a byte-identical copy of
`LICENSE` in every published crate, the CI write-scope inventory, the toolchain pin,
cross-revision registry continuity, the shape of each revision's spec page set
and of the draft tracking skeleton's (`sources.json`: every page in exactly one of
`in_scope` / `out_of_scope`, every exclusion reasoned), and the ecosystem register's row shape — are
exactly `cargo xtask gates`: they need nothing but a stable
toolchain and the checked-out tree, CI's `doc` job runs that one command, and
`cargo xtask ci` calls the same function — so what you run locally and what CI
runs cannot drift apart. They used to be written out in the workflow, and two of
them were left out of it. `cargo xtask falsify-gates` (also run by the `doc` job)
then breaks each of them, and the golden corpus, with one edit apiece on a clean
tree, and fails unless every one passes before its edit and goes red on it for the
reason it names, and unless the tree is clean again afterwards; a new gate without
a falsifier fails its unit test.

Additionally enforced in CI: `cargo package --workspace --exclude xtask --locked`
(publishability, with a CycloneDX SBOM dry run, `cargo xtask sbom`), the Rust API
against crates.io as the changelog's `[Unreleased]` section declares it
(`cargo xtask semver --unreleased`: while `[Unreleased]` declares no breaking
change, any API break fails the PR; once it carries a breaking-changes table,
every break is listed for the reviewer to hold against the table), the npm and
PyPI packages built from a release-shaped archive and installed and run on Linux,
macOS and Windows (`cargo xtask packages`, then `tools/test-packages.sh`, which
runs locally the same way), and
diff-scoped mutation testing
(`cargo mutants --in-diff`) on PRs. Coverage is measured and reported in the job
summary, and the job fails below 95% of lines. The standard is **zero surviving mutants in every shipped crate**
(xtask is excluded via `.cargo/mutants.toml`). Run it locally with
`cargo xtask mutants`, which issues exactly the invocation CI does;
`--jobs N` is the only argument it accepts, and raising it spends the timeout
headroom `.cargo/mutants.toml` reserves — a mutant merely slowed by
contention is reported as a timeout, not as caught.

## Working on checks and the corpus

The validator's credibility rests on two invariants — both enforced by tests:

1. **Registry ↔ inventory coverage**: every check the registry references exists, and
   every implemented check is referenced.
2. **Falsifiability**: every check is killed by at least one trace in
   `corpus/violations/` (`2025-11-25`) or `corpus/draft/violations/` (`2026-07-28`). A new check ships with a passing trace, a violating trace,
   and their golden reports.

Workflow: add the requirement to its area file under
`crates/mcp-conformance-core/registry/<revision>/` (verbatim spec quote — the tests
check the RFC 2119 keyword is present; gate it with a `capability` path when the
clause binds only after negotiation), implement the check, add corpus traces (`corpus/{good,violations}/` for
`2025-11-25`, `corpus/draft/{good,violations}/` for `2026-07-28`), then
`cargo xtask bless` and **review the golden diff like code**. A golden holds only
what its trace decided; the clauses the registry declines to judge live once per
revision in `corpus/golden/exclusions/`
([ADR-0013](docs/plan/decisions/0013-golden-report-format.md)), so a check that
takes over a previously excluded clause moves that ledger, `totals.excluded` in
every golden, and the clause's new row in each. Every corpus trace needs a row in
the provenance ledger, [`corpus/README.md`](corpus/README.md) — what produced it,
against which revision (an invariant test fails without one). Update
the README coverage table with `cargo xtask coverage`, and — when a capture or a
golden report changes — the corpus one with `cargo xtask draft-coverage`.

Do not write a coverage count by hand. Per
[ADR-0001](docs/plan/decisions/0001-plan-documentation-model.md)'s "one fact, one
place" rule these numbers are projections of the data: the per-area table comes
from the registry, the per-capture table from the committed golden reports, and
any prose saying "N of the M judgeable clauses" is parsed and checked against
them.

### Tracking the next revision

The specification's unreleased `draft/` is tracked, not judged: a skeleton in
`crates/mcp-conformance-core/registry/draft/` (`sources.json`, shaped like a dated
revision's, and `post-release.json`, the released clauses the draft rewords) that no
build embeds. `cargo xtask spec-drift --draft` (network) prints a Markdown report of what
the draft has changed — its page set and census against the skeleton, keywords no
released quote covers, and whether each `2026-07-28` quote is still on its draft page —
and exits 0 whatever it finds; the weekly `draft-drift` job puts the report in its job
summary. When it lists a change, read the draft page and update the skeleton: a census
number, a page decided into one of the two lists, or a `post-release.json` row for a
reworded clause (its `released` quote must equal the registry's, which `cargo test`
checks). [The conformance strategy](docs/plan/03-conformance-strategy.md#how-the-next-revision-starts)
says how the skeleton becomes the next revision when the draft ships.

## Commits

Conventional Commits with scopes, imperative mood, bodies that explain *why*:

```
feat(validator): add resources/subscribe ordering check
fix(core): reject registry entries with empty check lists
docs(plan): record the 2026-07-28 RC reconciliation outcome
ci(mutants): scope the PR gate to the diff
```

## Pull requests

The PR template checklist is the contract; the short version: SPDX header on every new
file, ≤ 500 lines per source file (`cargo xtask file-sizes`), rustdoc on new public items, tests for new code, ADR for
architectural decisions ([docs/plan/decisions/](docs/plan/decisions/README.md)), plan
documents updated when scope or status changed. No PR merges red — including docs-only
PRs, because docs build with `-D warnings` too.

## Reporting verdict disputes

If the validator's finding disagrees with your reading of the specification, file a
bug with the requirement ID and the spec text. Disputes are triaged into: our bug
(fixed), official-suite divergence (filed upstream), or spec ambiguity (filed
upstream). See [docs/plan/03-conformance-strategy.md](docs/plan/03-conformance-strategy.md).

## Security

Never report vulnerabilities in public issues — see [SECURITY.md](SECURITY.md).
