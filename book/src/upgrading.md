<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Upgrading

Before 1.0, a minor release (`0.6` → `0.7`) may change what the tools decide, not
only their Rust API: a check that learns to catch a defect fails a session that
passed, and a check that stops failing conforming sessions passes one that failed.
[STABILITY.md](https://github.com/tomtom215/mcp-conformance/blob/main/STABILITY.md)
says what counts as breaking. Every such change is in the release's changelog entry,
which since 0.5.0 opens with a table of each one and its migration (earlier entries
state them in their opening paragraphs).

## Before you upgrade

1. **Read the release's table.** Each row names the surface (a requirement ID, a
   report format, a flag), the change, and what to do:
   [Unreleased](https://github.com/tomtom215/mcp-conformance/blob/main/CHANGELOG.md#unreleased) (on `main`),
   [0.6.0](https://github.com/tomtom215/mcp-conformance/blob/main/CHANGELOG.md#060---2026-10-06),
   [0.5.0](https://github.com/tomtom215/mcp-conformance/blob/main/CHANGELOG.md#050---2026-08-26).
2. **Judge your recordings with both versions** and compare by requirement ID, not by
   totals: rows are added and moved between checked and excluded between releases, so
   the totals move even when nothing about your session's conformance did.

   ```text
   mcp-trace-validator validate --format json session.jsonl \
     | jq -r '.requirements[] | select(.outcome == "fail" or .outcome == "warn") | .id' \
     | sort > findings-new.txt
   ```

   The same command with the old binary gives `findings-old.txt`; `diff` the two. A
   row in one and not the other is a verdict change the table explains. The report's
   `wire_schema` member (on `main` since `0.6.0`) is judged beside the rows; compare
   its `outcome` too.
3. **Expect SARIF alerts to move once** where the table says a fingerprint changed;
   an alert whose finding is unchanged keeps its fingerprint.

## Pin the version you gate on

A CI gate should change its verdicts when you upgrade it, not when a release
happens. Pin it:

- `cargo install mcp-trace-validator --version 0.6.0 --locked`, or
  `cargo binstall mcp-trace-validator@0.6.0` for the attested prebuilt binary;
- the GitHub Action at a tag or commit (`uses: tomtom215/mcp-conformance@v0.6.0`),
  or with `version: 0.6.0` to download that release's prebuilt binaries rather than
  build from the action's source;
- `mcp-trace-validator = "=0.6.0"` for the library.

`mcp-trace-validator --version` names the build that judged a report; on `main`
since 0.6.0 it also names the protocol revisions that build judges.

## Library users

The Rust API follows the same release rule: a minor release may break it, the
changelog's table names every break, and `cargo-semver-checks` compares each release
with its predecessor on crates.io before it is published. A deprecated item stays for
at least one minor release, deprecated with its replacement named, before it is
removed. Types that will grow — reports, findings, outcomes, options — are
`#[non_exhaustive]`, so a new field or variant is not a break: match them with a
wildcard arm, and set options through their builder methods
(`HumanOptions::default().all(false)`) rather than struct literals.
