// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use serde_json::json;

use super::classify::{Category, compare, matches};
use super::convert::Frame;
use super::data::{Finding, Ours, Rule, Snoop, TraceResult};
use super::measure::{read_sarif, strip_prefix};
use super::{committed, verify};

fn rule(id: &str, template: &str, ids: &[&str], partial: &[&str], context: Option<&str>) -> Rule {
    Rule {
        id: id.to_owned(),
        templates: vec![template.to_owned()],
        maps: BTreeMap::from([(
            "2026-07-28".to_owned(),
            ids.iter().map(|id| (*id).to_owned()).collect(),
        )]),
        partial: partial.iter().map(|id| (*id).to_owned()).collect(),
        context: context.map(str::to_owned),
        reading: String::new(),
    }
}

fn trace(flagged: &[&str], findings: &[(&str, bool)], unframed: usize) -> TraceResult {
    TraceResult {
        trace: "draft/violations/x".to_owned(),
        revision: "2026-07-28".to_owned(),
        transport: "http".to_owned(),
        frames: 4,
        unframed,
        ours: Ours {
            verdict: if flagged.is_empty() { "pass" } else { "fail" }.to_owned(),
            flagged: flagged.iter().map(|id| (*id).to_owned()).collect(),
        },
        mcpsnoop: Snoop {
            exit: i32::from(!findings.is_empty()),
            findings: findings
                .iter()
                .map(|(message, framed)| Finding {
                    signal: "warn".to_owned(),
                    seq: Some(1),
                    framed: *framed,
                    message: (*message).to_owned(),
                })
                .collect(),
            other: BTreeMap::new(),
        },
    }
}

fn rules() -> Vec<Rule> {
    vec![
        rule(
            "in-flight",
            "request reuses an id already in flight",
            &["BASE-045"],
            &[],
            Some("pairing"),
        ),
        rule(
            "ttl",
            "cacheable * result declares no ttlMs",
            &["CACH-001"],
            &["CACH-001"],
            None,
        ),
        rule(
            "version-header",
            "required routing header MCP-Protocol-Version is missing",
            &["TRAN-071"],
            &[],
            Some("http"),
        ),
    ]
}

#[test]
fn templates_match_in_order_with_wildcards() {
    assert!(matches(
        "cacheable * result declares no ttlMs",
        "cacheable tools/list result declares no ttlMs, which 2026-07-28 requires"
    ));
    assert!(!matches(
        "cacheable * result declares no ttlMs",
        "result declares no ttlMs on a cacheable list"
    ));
    assert!(matches("exact", "an exact phrase"));
}

#[test]
fn the_session_prefix_is_stripped() {
    assert_eq!(
        strip_prefix("session mcp-conformance:a/b frame 3: request reuses an id"),
        "request reuses an id"
    );
    assert_eq!(
        strip_prefix("session s: tool \"x\" schema uses ref"),
        "tool \"x\" schema uses ref"
    );
    assert_eq!(strip_prefix("no prefix: here"), "no prefix: here");
}

#[test]
fn sarif_findings_are_tied_back_to_trace_events() {
    let sarif = json!({"runs": [{"results": [
        {"ruleId": "mcpsnoop/warn", "message": {"text": "session s frame 2: jsonrpc must be 2.0"},
         "locations": [{"physicalLocation": {"region": {"startLine": 2}}}]},
        {"ruleId": "mcpsnoop/pending", "message": {"text": "session s frame 2: ping (id 1) never got a response"}},
        {"ruleId": "mcpsnoop/error", "message": {"text": "x"}}
    ]}]});
    let frames = [
        Frame {
            seq: None,
            framed: true,
        },
        Frame {
            seq: Some(7),
            framed: false,
        },
    ];
    let snoop = read_sarif(&sarif, &frames, 1).unwrap();
    assert_eq!(
        snoop.findings.len(),
        1,
        "only conformance signals are kept as findings"
    );
    assert_eq!(snoop.findings[0].seq, Some(7));
    assert!(!snoop.findings[0].framed);
    assert_eq!(snoop.findings[0].message, "jsonrpc must be 2.0");
    assert_eq!(
        snoop.other,
        BTreeMap::from([("error".to_owned(), 1), ("pending".to_owned(), 1)])
    );
}

#[test]
fn readings_are_classified() {
    let rules = rules();
    let clean = trace(&[], &[], 0);
    assert_eq!(compare(&clean, &rules).category(), Category::Clean);
    let agree = trace(
        &["BASE-045"],
        &[("request reuses an id already in flight", true)],
        0,
    );
    assert_eq!(compare(&agree, &rules).category(), Category::Agree);
    let snoop_only = trace(&[], &[("request reuses an id already in flight", true)], 0);
    let comparison = compare(&snoop_only, &rules);
    assert_eq!(comparison.category(), Category::Divergent);
    assert_eq!(comparison.divergences(), [("mcpsnoop-only", "in-flight")]);
    let ours_only = trace(&["BASE-045"], &[], 0);
    assert_eq!(
        compare(&ours_only, &rules).divergences(),
        [("ours-only", "BASE-045")]
    );
    // A partial mapping's silence says nothing; a clause no rule maps is scope.
    let beyond = trace(&["CACH-001", "TOOL-019"], &[], 0);
    let comparison = compare(&beyond, &rules);
    assert_eq!(comparison.category(), Category::Beyond);
    assert_eq!(comparison.beyond, ["CACH-001", "TOOL-019"]);
}

#[test]
fn findings_reading_absent_context_are_set_aside() {
    let rules = rules();
    let unframed = trace(
        &[],
        &[(
            "required routing header MCP-Protocol-Version is missing",
            false,
        )],
        1,
    );
    let comparison = compare(&unframed, &rules);
    assert_eq!(comparison.category(), Category::Clean);
    assert!(comparison.set_aside.contains("version-header"));
    // A pairing rule is set aside anywhere in a trace with an unframed frame,
    // and answers the clause rather than leaving it ours-only.
    let pairing = trace(
        &["BASE-045"],
        &[("request reuses an id already in flight", true)],
        2,
    );
    let comparison = compare(&pairing, &rules);
    assert!(comparison.fired.is_empty());
    assert_eq!(comparison.divergences(), [] as [(&str, &str); 0]);
    let unclaimed = trace(&[], &[("something new", true)], 0);
    assert_eq!(compare(&unclaimed, &rules).unclaimed, ["something new"]);
}

#[test]
fn the_committed_page_is_what_the_committed_files_render() {
    let root = crate::workspace_root();
    let inputs = committed(&root).unwrap();
    let rendered = verify::verify(&root, &inputs).unwrap();
    let page = std::fs::read_to_string(root.join(super::PAGE)).unwrap();
    assert_eq!(rendered, page);
}

#[test]
fn stale_and_missing_triage_are_refused() {
    let root = crate::workspace_root();
    let mut inputs = committed(&root).unwrap();
    let removed = inputs.triage.divergences.remove(0);
    let problems = verify::verify(&root, &inputs).unwrap_err();
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("has no triage entry")),
        "{problems:?}"
    );
    let mut stale = removed;
    stale.traces.push("good/stdio-full-session".to_owned());
    inputs.triage.divergences.insert(0, stale);
    let problems = verify::verify(&root, &inputs).unwrap_err();
    assert!(
        problems.iter().any(|problem| problem.contains("(stale)")),
        "{problems:?}"
    );
}
