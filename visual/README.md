<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Screenshot suite

What people see of `mcp-trace-validator validate --format html` and of the workbench
(`mcp-conformance serve`), verified the way
[ADR-0021](../docs/plan/decisions/0021-a-workbench-people-keep-open.md) ("Verification of
what people see") asks: a headless Chromium (Playwright, pinned) renders committed pages
and compares them with committed screenshots, in CI.

The pages are the golden HTML reports in [`corpus/golden/html/`](../corpus/golden/html),
which `cargo test -p mcp-trace-validator --test golden_html` pins byte for byte against the
renderer. So the chain from the Rust renderer to pixels has no unpinned link, and this
suite needs no Rust toolchain.

[`tests/report.spec.js`](tests/report.spec.js) checks:

- a passing (`http-session`), a failing (`base-001-request-id-boolean`) and a two-revision
  (`disc-002-dual-era-client-skips-probe`) report, at 1280 px and 360 px, in light and dark,
  against [`tests/__screenshots__/`](tests/__screenshots__) with no pixel allowed to differ;
- that opening any of them makes **no request** (every request other than the document is
  recorded and aborted; the list must be empty), logs no console error (where a Content
  Security Policy violation is reported, so a wrong stylesheet digest fails here), and does
  not scroll horizontally;
- that a finding and its event link to each other, that the "only events with findings"
  switch works with no script on the page, and that the accounting opens.

## The workbench

[`tests/workbench-views.spec.js`](tests/workbench-views.spec.js) and
[`tests/workbench-interactions.spec.js`](tests/workbench-interactions.spec.js) do the same
for `mcp-conformance serve` ([ADR-0021](../docs/plan/decisions/0021-a-workbench-people-keep-open.md),
phase W5). [`tests/workbench/serve.js`](tests/workbench/serve.js) starts the binary over a
fresh copy of the committed fixture library, [`fixtures/library/`](fixtures/library) — eleven
small recordings from the corpus plus two written for the workbench: `hostile-markup` (markup,
an escape character and a bidirectional override in method names, ids, names and payloads) and
`leaky-session` (a trace another recorder made, carrying an `Authorization` header, a cookie, a
tool argument and a `_meta` value that must not survive redaction; the values are obviously
fake). It exchanges the single-use launch link for the session cookie, as a browser does, and
gives each test's browser context that cookie.

- Views: the library, a session opened at a finding's event, the hostile session, two sessions
  compared, and the dashboards, each at 1280 px and 360 px in light and dark, compared with
  `__screenshots__/workbench-*.png`. Every request that is not to the workbench's own origin
  fails the test (and is aborted), as does any console error or warning (a CSP violation is
  one) and any horizontal scrolling.
- The hostile session renders as text: every payload opened, no `img`, `script`, `iframe` or
  `svg` on the page, no dialog, and the escape and bidi characters shown as `\u{…}`.
- Interactions, each against its own server: search (field terms, negation, prefixes, a query
  in the address, a malformed query), finding → event → finding, the diff of two servers,
  redaction (the copy and its sidecar hold none of the four secrets, the original is
  unchanged), delete, an mcpsnoop import, export, retention, following a trace as lines are
  appended to it, and the launch link refusing a second use.

The binary is `$MCP_CONFORMANCE_BIN`, else `target/debug/mcp-conformance` at the repository
root; CI builds it in the `visual` job.

## Running it

```sh
cargo test -p mcp-trace-validator --test golden_html   # the pages are current
cd visual
npm ci
npx playwright install --with-deps chromium             # once; CI does this every run
cargo build -p mcp-conformance-cli                       # the workbench the specs serve
npm test
```

After a deliberate visual change: `cargo xtask bless` (the golden pages), then new
screenshots, and review both diffs — the screenshots as images. The committed
screenshots are the CI runner's rendering, not a local one: the same Chromium build on
another machine still rasterises text differently (on 2026-10-13 a sandbox's
rendering of identical pages differed from the runner's in 2.4% of pixels, all of it
glyph anti-aliasing). So take new baselines from the `visual` job's `visual-diffs`
artifact: each failed comparison uploads its `*-actual.png`, which is the runner's
rendering of the new page. `npm run bless` is for looking locally, not for committing.

## Determinism

Baselines are taken on Linux (Ubuntu 24.04, the CI runner) with `fonts-dejavu-core`
installed. The report's font stacks put DejaVu Sans and DejaVu Sans Mono first, because
Linux desktops alias the platform UI names (`system-ui`, `Segoe UI`, `Helvetica Neue`, even
`sans-serif`) to whatever font each machine prefers; DejaVu is the same everywhere
fontconfig is. The report ships no font — it must render as shipped — so the font is
pinned in the environment, not in the page. On another platform, or with another
Chromium, expect differences: the baselines are only compared on Linux.

`@playwright/test` is pinned exactly (`package.json`, `package-lock.json`); its version
determines the Chromium build. `shoot.mjs` takes ad-hoc screenshots of any report for a look
before baselines move; it is not part of the suite.
