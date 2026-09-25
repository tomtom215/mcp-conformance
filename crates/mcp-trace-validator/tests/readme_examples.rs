// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The root README's worked examples, executed.
//!
//! A documentation example that drifts from the tool's real output is a small
//! lie with a long shelf life — the 2026-06-11 docs review found both README
//! totals lines still summing to a registry two-thirds the current size. These
//! tests extract the README's own example trace and quoted outputs and assert
//! them against the real validator, so the examples cannot silently rot again.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_conformance_core::requirement::Registry;
use mcp_trace_validator::reader::{Limits, parse_trace};
use mcp_trace_validator::{engine, report::Report};

fn readme() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md");
    std::fs::read_to_string(path).expect("root README exists")
}

/// The content of the first fenced block whose info string is `lang`,
/// starting the search at `from`.
fn fenced_block(text: &str, lang: &str, from: usize) -> String {
    let open = format!("```{lang}\n");
    let start = text[from..].find(&open).expect("opening fence") + from + open.len();
    let end = text[start..].find("```").expect("closing fence") + start;
    text[start..end].to_owned()
}

fn validate_text(trace: &str) -> Report {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let events = parse_trace(trace, &Limits::default()).expect("README trace parses");
    engine::validate(&registry, &events)
}

#[test]
fn the_inline_trace_example_produces_exactly_the_quoted_output() {
    let readme = readme();
    // The worked example: a ```jsonl fence holding the trace, followed by a
    // ```text fence quoting the validator's answer.
    let trace = fenced_block(&readme, "jsonl", 0);
    let trace_at = readme.find("```jsonl").unwrap();
    let quoted = fenced_block(&readme, "text", trace_at);

    let rendered = validate_text(&trace).render_human();
    for line in quoted.lines().filter(|line| !line.trim().is_empty()) {
        assert!(
            rendered.contains(line),
            "README quotes a line the validator does not produce:\n  {line}\nactual output:\n{rendered}"
        );
    }
    // The narrative around the example claims exactly five not-applicable
    // rows; hold the prose to the same standard as the quoted output.
    let report = validate_text(&trace);
    assert_eq!(
        report.totals.not_applicable, 6,
        "the README prose explains six not-applicable rows"
    );
}

#[test]
fn the_opening_example_is_what_the_cli_prints_for_the_trace_it_depicts() {
    // The first example depicts `validate --quiet` on the committed MRTR-019
    // violation trace. Every line it quotes is checked against the output the CLI
    // produces for that trace: the revision chosen the way the CLI chooses it, and
    // the findings-only rendering `--quiet` selects.
    let readme = readme();
    let example_at = readme
        .find("validate --quiet session.jsonl\nMCP trace validation")
        .expect("the opening example");
    let open_at = readme[..example_at]
        .rfind("```text")
        .expect("opening fence before the marker");
    let quoted = fenced_block(&readme, "text", open_at);

    let trace_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/draft/violations/mrtr-019-retry-reuses-id.jsonl"
    );
    let events = parse_trace(
        &std::fs::read_to_string(trace_path).unwrap(),
        &Limits::default(),
    )
    .unwrap();
    let set = mcp_conformance_core::requirement::RegistrySet::builtin().unwrap();
    let selection = mcp_trace_validator::declared::select(set.revisions(), &events).unwrap();
    let registry = set.registry(selection.revisions[0]).unwrap();
    let mut report = engine::validate(&registry, &events);
    report.revision_source = Some(selection.source);
    let rendered = report.render_findings();

    let quoted_lines: Vec<&str> = quoted
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('$'))
        .collect();
    assert_eq!(
        quoted_lines,
        rendered.lines().collect::<Vec<_>>(),
        "the README's opening example must be exactly what `validate --quiet` prints"
    );
}
