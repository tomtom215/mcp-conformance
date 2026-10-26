// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The matrix block itself: a table row per SDK and revision, then every
//! finding the committed reports carry.
//!
//! Counts sit in their own columns rather than in a "N pass, M fail" phrase on
//! purpose: that phrase is the shape `draft-coverage` reads as a claim about
//! the `2026-07-28` captures, and half of these rows are `2025-11-25` reports.

use core::fmt::Write as _;
use std::collections::BTreeMap;

use serde_json::Value;

use super::{REVISIONS, Revision, SDKS, Sdk, stem};

/// Where committed files are linked from: the book is published away from the
/// repository, so a relative path into `corpus/` would not resolve there.
const BLOB: &str = "https://github.com/tomtom215/mcp-conformance/blob/main";

/// The generated block, ending in a newline.
pub(super) fn block(pins: &BTreeMap<String, String>, cells: &[Option<Value>]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "| SDK | Pinned release | Revision | Verdict | pass | fail | warn | not observed | Wire schema | Evidence |"
    );
    let _ = writeln!(
        out,
        "|-----|----------------|----------|---------|-----:|-----:|-----:|-------------:|-------------|----------|"
    );
    let mut findings = String::new();
    let mut cell = cells.iter();
    for sdk in &SDKS {
        let release = format!("`{}` {}", sdk.package, pins[sdk.pin]);
        for revision in &REVISIONS {
            let Some(report) = cell.next().and_then(Option::as_ref) else {
                let _ = writeln!(
                    out,
                    "| {} | {release} | `{}` | not served | — | — | — | — | — | — |",
                    sdk.name, revision.id
                );
                continue;
            };
            row(&mut out, sdk, &release, revision, report);
            for line in finding_lines(report, revision.id) {
                let _ = writeln!(findings, "- **{}, `{}`** — {line}", sdk.name, revision.id);
            }
        }
    }
    out.push('\n');
    for revision in &REVISIONS {
        let _ = writeln!(
            out,
            "At `{}` every server is driven by the same {}.",
            revision.id, revision.client
        );
    }
    out.push('\n');
    if findings.is_empty() {
        out.push_str("No committed report carries a finding.\n");
    } else {
        out.push_str(
            "**Findings.** Every failing or warning row of a committed report, with the \
             first of its findings as the validator words it; the report link holds the rest.\n\n",
        );
        out.push_str(&findings);
    }
    out
}

/// The table row of one recorded session.
fn row(out: &mut String, sdk: &Sdk, release: &str, revision: &Revision, report: &Value) {
    let stem = stem(sdk, revision);
    let totals = &report["totals"];
    let verdict = report["verdict"].as_str().unwrap_or("?");
    let verdict = if verdict == "pass" {
        verdict.to_owned()
    } else {
        format!("**{verdict}**")
    };
    let wire = &report["wire_schema"];
    let _ = writeln!(
        out,
        "| {} | {release} | `{}` | {verdict} | {} | {} | {} | {} | {} ({} messages) | [report]({BLOB}/{}/{stem}.json) · [trace]({BLOB}/{}/{stem}.jsonl) |",
        sdk.name,
        revision.id,
        totals["pass"],
        totals["fail"],
        totals["warn"],
        totals["not_observed"],
        wire["outcome"].as_str().unwrap_or("?"),
        wire["messages"],
        revision.goldens,
        revision.traces,
    );
}

/// One line per failing or warning requirement, then the wire schema's, if it
/// failed.
fn finding_lines(report: &Value, revision: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for row in report["requirements"].as_array().into_iter().flatten() {
        let outcome = row["outcome"].as_str().unwrap_or("");
        if outcome != "fail" && outcome != "warn" {
            continue;
        }
        let id = row["id"].as_str().unwrap_or("?");
        lines.push(format!(
            "[`{id}`](reference/requirements-{revision}.md#{}) ({}, {outcome}, {}): {}",
            id.to_lowercase(),
            row["level"].as_str().unwrap_or("?"),
            plural(row["findings"].as_array().map_or(0, Vec::len)),
            first_detail(&row["findings"]),
        ));
    }
    let wire = &report["wire_schema"];
    if wire["outcome"] == "fail" {
        lines.push(format!(
            "wire schema (fail, {}): {}",
            plural(wire["findings"].as_array().map_or(0, Vec::len)),
            first_detail(&wire["findings"]),
        ));
    }
    lines
}

/// `1 finding`, `3 findings`.
fn plural(count: usize) -> String {
    if count == 1 {
        "1 finding".to_owned()
    } else {
        format!("{count} findings")
    }
}

/// The first finding's detail, on one line.
fn first_detail(findings: &Value) -> String {
    findings[0]["detail"]
        .as_str()
        .unwrap_or("(no detail)")
        .replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pins() -> BTreeMap<String, String> {
        SDKS.iter()
            .map(|sdk| (sdk.pin.to_owned(), "1.0.0".to_owned()))
            .collect()
    }

    #[test]
    fn an_absent_cell_reads_not_served_and_a_finding_is_listed() {
        let failing = json!({
            "revision": "2026-07-28",
            "verdict": "fail",
            "totals": {"pass": 3, "fail": 1, "warn": 0, "not_observed": 2},
            "requirements": [
                {"id": "BASE-034", "level": "MUST NOT", "outcome": "fail",
                 "findings": [{"check": "c", "seq": 4, "detail": "first"},
                              {"check": "c", "seq": 9, "detail": "second"}]},
                {"id": "BASE-035", "level": "MUST", "outcome": "pass"}
            ],
            "wire_schema": {"outcome": "pass", "messages": 7}
        });
        let mut cells = vec![None; SDKS.len() * REVISIONS.len()];
        cells[1] = Some(failing);
        let rendered = block(&pins(), &cells);
        assert!(rendered.contains(
            "| TypeScript v1 | `@modelcontextprotocol/sdk` 1.0.0 | `2025-11-25` | not served |"
        ));
        assert!(
            rendered.contains("| `2026-07-28` | **fail** | 3 | 1 | 0 | 2 | pass (7 messages) |")
        );
        assert!(rendered.contains(
            "- **TypeScript v1, `2026-07-28`** — [`BASE-034`](reference/requirements-2026-07-28.md#base-034) (MUST NOT, fail, 2 findings): first"
        ));
        assert!(
            !rendered.contains("BASE-035"),
            "a passing row is not a finding"
        );
    }

    #[test]
    fn a_clean_matrix_says_so() {
        let cells = vec![None; SDKS.len() * REVISIONS.len()];
        assert!(block(&pins(), &cells).contains("No committed report carries a finding."));
    }
}
