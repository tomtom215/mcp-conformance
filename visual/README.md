<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Screenshot suite

What people see of `mcp-trace-validator validate --format html`, verified the way
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

## Running it

```sh
cargo test -p mcp-trace-validator --test golden_html   # the pages are current
cd visual
npm ci
npx playwright install --with-deps chromium             # once; CI does this every run
npm test
```

After a deliberate visual change: `cargo xtask bless` (the golden pages), then
`npm run bless` (the screenshots), and review both diffs — the screenshots as images.

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
