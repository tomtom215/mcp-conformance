<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

## What and why

<!-- One paragraph: the change, and the reason it exists. Link issues. -->

## Checklist

- [ ] `cargo xtask ci` passes locally (format, clippy × feature modes, tests, docs)
- [ ] SPDX header on every new file
- [ ] No file exceeds 500 lines
- [ ] New public items have rustdoc (with runnable examples on entry points)
- [ ] New code has tests; new checks have both a passing and a violating corpus trace
- [ ] Golden reports regenerated deliberately (`cargo xtask bless`) if behavior changed, diff reviewed
- [ ] ADR added/updated if an architectural decision was made or revised (`docs/plan/decisions/`)
- [ ] Plan documents updated if scope, roadmap status, or verified facts changed
