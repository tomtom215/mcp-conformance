// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::reader::{Limits, parse_trace};
use mcp_conformance_core::requirement::Registry;

const HAPPY: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}
{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;

#[test]
fn happy_path_passes_every_checked_requirement() {
    use mcp_conformance_core::requirement::Verification;
    let registry = Registry::builtin_2025_11_25().unwrap();
    let events = parse_trace(HAPPY, &Limits::default()).unwrap();
    let report = validate(&registry, &events);
    assert!(!report.has_errors(), "{}", report.render_human());
    assert!(!report.has_warnings(), "{}", report.render_human());
    let documented_exclusions = registry
        .requirements()
        .iter()
        .filter(|requirement| matches!(requirement.verification, Verification::Excluded { .. }))
        .count();
    assert_eq!(
        usize::try_from(report.totals.excluded).unwrap(),
        documented_exclusions,
        "every documented exclusion reports as excluded, regardless of trace"
    );
    // This handshake declares no capabilities, so every gated requirement
    // must surface as not-applicable — never as a vacuous pass.
    let gated = registry
        .requirements()
        .iter()
        .filter(|requirement| {
            requirement.capability.is_some()
                && matches!(requirement.verification, Verification::Checks { .. })
        })
        .count();
    assert_eq!(
        usize::try_from(report.totals.not_applicable).unwrap(),
        gated,
        "{}",
        report.render_human()
    );
    assert_eq!(report.totals.unsupported, 0);
    // A bare handshake exercises almost nothing, and the report says so
    // rather than crediting the session with clauses it never approached.
    // PAGE-002 is the plainest case: no listing was ever paginated, so no
    // cursor was ever presented for the opacity rule to bind to.
    let pagination = report
        .requirements
        .iter()
        .find(|row| row.id == "PAGE-002")
        .expect("the 2025-11-25 registry carries PAGE-002");
    assert_eq!(
        pagination.outcome,
        Outcome::NotObserved,
        "{}",
        report.render_human()
    );
    assert_eq!(
        usize::try_from(
            report.totals.pass
                + report.totals.fail
                + report.totals.warn
                + report.totals.excluded
                + report.totals.unsupported
                + report.totals.not_applicable
                + report.totals.not_observed
        )
        .unwrap(),
        registry.requirements().len(),
        "every requirement is accounted for exactly once"
    );
}

/// One-requirement registry gated on `server.tools`, with a real check.
const GATED_REGISTRY: &str = r#"{
    "revision": "2025-11-25",
    "requirements": [
        {"id": "TOOL-001", "level": "MUST", "actor": "server",
         "capability": "server.tools",
         "source": {"section": "server/tools#x", "quote": "MUST t"},
         "checks": ["base.jsonrpc-version"]}
    ]
}"#;

fn handshake(server_capabilities: &str) -> String {
    format!(
        r#"{{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"t","version":"0"}}}}}}}}
{{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{server_capabilities},"serverInfo":{{"name":"s","version":"0"}}}}}}}}
{{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","method":"notifications/initialized"}}}}"#
    )
}

#[test]
fn undeclared_capability_reports_not_applicable_not_pass() {
    let registry = Registry::from_json(GATED_REGISTRY).unwrap();
    let trace = handshake(r#"{"prompts":{}}"#);
    let events = parse_trace(&trace, &Limits::default()).unwrap();
    let report = validate(&registry, &events);
    assert_eq!(report.totals.not_applicable, 1);
    assert_eq!(report.totals.pass, 0);
    assert_eq!(
        report.requirements[0].outcome,
        crate::report::Outcome::NotApplicable
    );
    assert_eq!(
        report.requirements[0].capability.as_deref(),
        Some("server.tools")
    );
    assert_eq!(report.verdict(), crate::report::Verdict::Pass);
}

#[test]
fn declared_capability_runs_the_gated_checks() {
    let registry = Registry::from_json(GATED_REGISTRY).unwrap();
    let trace = handshake(r#"{"tools":{"listChanged":true}}"#);
    let events = parse_trace(&trace, &Limits::default()).unwrap();
    let report = validate(&registry, &events);
    assert_eq!(report.totals.not_applicable, 0);
    assert_eq!(report.totals.pass, 1);
    assert!(report.requirements[0].capability.is_none());
}

#[test]
fn missing_checks_outrank_the_capability_gate() {
    // `unsupported` must be a property of (registry, build), not of what one
    // trace negotiated — a gated requirement with an unknown check is
    // unsupported even when the capability was never declared.
    let registry_json = r#"{
        "revision": "2025-11-25",
        "requirements": [
            {"id": "TOOL-001", "level": "MUST", "actor": "server",
             "capability": "server.tools",
             "source": {"section": "server/tools#x", "quote": "MUST t"},
             "checks": ["future.not-built-yet"]}
        ]
    }"#;
    let registry = Registry::from_json(registry_json).unwrap();
    let report = validate(&registry, &[]);
    assert_eq!(report.totals.unsupported, 1);
    assert_eq!(report.totals.not_applicable, 0);
}

#[test]
fn unknown_check_reports_unsupported_not_silence() {
    let registry_json = r#"{
        "revision": "2025-11-25",
        "requirements": [
            {"id": "FUTR-001", "level": "MUST", "actor": "both",
             "source": {"section": "future#x", "quote": "MUST do future things"},
             "checks": ["future.not-built-yet"]}
        ]
    }"#;
    let registry = Registry::from_json(registry_json).unwrap();
    let report = validate(&registry, &[]);
    assert_eq!(report.totals.unsupported, 1);
    assert!(report.has_unsupported());
    assert_eq!(
        report.requirements[0].missing_checks,
        ["future.not-built-yet"]
    );
}

#[test]
fn empty_trace_passes_vacuously_with_gates_not_applicable() {
    // The deliberate verdict for "nothing happened": no clause was
    // violated, so the trace passes — while every capability-gated
    // requirement reports not-applicable rather than a vacuous pass,
    // and the totals make the vacuity visible. (Whether an *empty
    // session* is acceptable evidence is the caller's question: the
    // agreement check, for one, rejects empty tap directories.)
    let registry = Registry::builtin_2025_11_25().unwrap();
    let report = validate(&registry, &[]);
    assert_eq!(report.verdict(), crate::report::Verdict::Pass);
    assert_eq!(report.totals.fail, 0);
    assert_eq!(report.totals.warn, 0);
    assert_eq!(report.totals.unsupported, 0);
    let gated = registry
        .requirements()
        .iter()
        .filter(|requirement| {
            requirement.capability.is_some()
                && matches!(
                    requirement.verification,
                    mcp_conformance_core::requirement::Verification::Checks { .. }
                )
        })
        .count();
    assert_eq!(
        usize::try_from(report.totals.not_applicable).unwrap(),
        gated
    );
}

#[test]
fn failing_and_warning_rows_carry_their_clause_and_no_other_row_does() {
    let registry = Registry::from_json(
        r#"{
        "revision": "2026-07-28",
        "requirements": [
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "basic/index#messages", "quote": "MUST be JSON-RPC 2.0"},
             "checks": ["base.jsonrpc-version"]},
            {"id": "BASE-002", "level": "SHOULD", "actor": "both",
             "source": {"section": "basic/lifecycle", "quote": "SHOULD be JSON-RPC 2.0"},
             "checks": ["base.jsonrpc-version"]},
            {"id": "BASE-003", "level": "MUST", "actor": "both",
             "source": {"section": "basic/index#requests", "quote": "MUST have an id"},
             "checks": ["base.request-id-type"]},
            {"id": "BASE-004", "level": "MUST", "actor": "both",
             "source": {"section": "basic/index#auth", "quote": "MUST authorize"},
             "exclusion": "not observable"}
        ]
    }"#,
    )
    .unwrap();
    let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"1.0","id":1,"method":"ping"}}"#;
    let report = validate(&registry, &parse_trace(trace, &Limits::default()).unwrap());
    let sources: Vec<_> = report
        .requirements
        .iter()
        .map(|row| {
            (
                row.outcome,
                row.source.as_ref().map(|source| source.url.as_str()),
            )
        })
        .collect();
    assert_eq!(
        sources,
        [
            (
                Outcome::Fail,
                Some("https://modelcontextprotocol.io/specification/2026-07-28/basic#messages")
            ),
            (
                Outcome::Warn,
                Some("https://modelcontextprotocol.io/specification/2026-07-28/basic/lifecycle")
            ),
            (Outcome::Pass, None),
            (Outcome::Excluded, None),
        ]
    );
    let failed = report.requirements[0].source.as_ref().unwrap();
    assert_eq!(failed.section, "basic/index#messages");
    assert_eq!(failed.quote, "MUST be JSON-RPC 2.0");
}

#[test]
fn report_is_deterministic_across_runs() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let events = parse_trace(HAPPY, &Limits::default()).unwrap();
    let a = serde_json::to_string(&validate(&registry, &events)).unwrap();
    let b = serde_json::to_string(&validate(&registry, &events)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn try_validate_reports_events_out_of_order_instead_of_panicking() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let events = parse_trace(HAPPY, &Limits::default()).unwrap();
    assert_eq!(
        try_validate(&registry, &events).unwrap(),
        validate(&registry, &events),
        "ordered events give exactly validate's report"
    );
    let mut repeated = events.clone();
    repeated[2] = repeated[1].clone();
    assert_eq!(
        try_validate(&registry, &repeated).unwrap_err(),
        SeqOrderError {
            index: 2,
            seq: 1,
            previous: 1
        },
        "a repeated seq is out of order"
    );
    let mut swapped = events;
    swapped.swap(0, 1);
    let error = try_validate(&registry, &swapped).unwrap_err();
    assert_eq!((error.index, error.seq, error.previous), (1, 0, 1));
    assert_eq!(
        error.to_string(),
        "event 1 has seq 0, not greater than the preceding seq 1; trace events must have \
         strictly increasing seq"
    );
}

/// The pre-0.6.0 path still names the same items, for one minor release.
#[test]
#[allow(deprecated)]
fn the_deprecated_draft_path_names_the_stateless_items() {
    fn same<T>(_: fn() -> T, _: fn() -> T) {}
    same::<Option<crate::context::draft::DraftPhase>>(
        || None,
        || None::<crate::context::stateless::DraftPhase>,
    );
}
