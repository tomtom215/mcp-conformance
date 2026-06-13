// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Pathological-input boundedness: the validator must stay linear-ish and
//! allocation-sane on inputs far beyond real sessions, and reject what its
//! parsers cannot bound. These are correctness tests (they complete or they
//! don't), not benchmarks — benches/README.md records why no timing gate
//! exists. Honest limit: a mutant that is quadratic *but correct* passes
//! here unless it also blows cargo-mutants' auto-timeout; only verdict
//! changes and hangs are caught, by design.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_conformance_core::requirement::Registry;
use mcp_trace_validator::report::Verdict;
use mcp_trace_validator::{engine, reader};
use std::fmt::Write as _;

/// A conformant session padded to `pings` request/response pairs.
fn long_session(pings: usize) -> String {
    let mut doc = String::new();
    doc.push_str(r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#);
    doc.push('\n');
    doc.push_str(r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#);
    doc.push('\n');
    doc.push_str(r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#);
    let mut seq = 3;
    for ping in 0..pings {
        let id = ping + 2;
        write!(
            doc,
            "\n{{\"seq\":{seq},\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"ping\"}}}}"
        )
        .unwrap();
        write!(
            doc,
            "\n{{\"seq\":{},\"direction\":\"server-to-client\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}}}}}}",
            seq + 1
        )
        .unwrap();
        seq += 2;
    }
    doc
}

#[test]
fn one_hundred_thousand_events_validate_within_test_patience() {
    // 100k events ≈ 100× any real tapped session. The assertion is
    // completion with the right verdict and totals: a superlinear pairing or
    // check (the 10^10-step kind) would hang the suite loudly instead of
    // shipping, and memory stays proportional to the document.
    let document = long_session(50_000 - 2); // 3 + 2·(50_000−2) ≈ 100k events
    let events = reader::parse_trace(&document, &reader::Limits::default())
        .expect("a long conformant session parses");
    assert!(events.len() > 99_000);
    let registry = Registry::builtin_2025_11_25().unwrap();
    let report = engine::validate(&registry, &events);
    assert_eq!(
        report.verdict(),
        Verdict::Pass,
        "conformant remains conformant at scale:\n{}",
        report.render_human()
    );
}

#[test]
fn pathological_id_reuse_stays_linear_and_is_judged() {
    // Every request reuses id 7: the worst case for any id-keyed pairing
    // structure. Must complete and must flag the reuse, not degrade into
    // quadratic rescans or collapse the duplicates silently.
    let mut doc = String::new();
    doc.push_str(r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#);
    doc.push('\n');
    doc.push_str(r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#);
    doc.push('\n');
    doc.push_str(r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#);
    for index in 0..20_000u32 {
        write!(
            doc,
            "\n{{\"seq\":{},\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}}}}",
            index + 3
        )
        .unwrap();
    }
    let events = reader::parse_trace(&doc, &reader::Limits::default()).unwrap();
    let registry = Registry::builtin_2025_11_25().unwrap();
    let report = engine::validate(&registry, &events);
    let reuse = report
        .requirements
        .iter()
        .find(|row| row.id == "BASE-003")
        .expect("BASE-003 row");
    assert!(
        !reuse.findings.is_empty(),
        "mass id reuse must be flagged, not absorbed"
    );
}

#[test]
fn deeply_nested_payload_is_rejected_at_parse_with_a_named_line() {
    // serde_json bounds recursion; a hostile 10k-deep payload must surface
    // as a typed parse error naming the line — never a stack overflow and
    // never a judged-anyway trace.
    let depth = 10_000;
    let payload = format!("{}\"x\"{}", "[".repeat(depth), "]".repeat(depth));
    let line = format!(
        r#"{{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{payload}}}"#
    );
    let error = reader::parse_trace(&line, &reader::Limits::default())
        .expect_err("hostile nesting must not parse");
    let message = error.to_string();
    assert!(
        message.contains("line 1"),
        "the error names the offending line: {message}"
    );
}
