// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for `lifecycle.fallback-version-latest`.

#![allow(clippy::expect_used)]

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

const CHECK: &str = "lifecycle.fallback-version-latest";

fn line(seq: usize, direction: &str, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"streamable-http","kind":"message","payload":{payload}}}"#
    )
}

/// A trace of `initialize` exchanges: each `(requested, answered)` is the JSON
/// of the request's and the result's `protocolVersion`, or an empty string to
/// leave the member out.
fn trace(exchanges: &[(&str, &str)]) -> String {
    let member = |version: &str| {
        if version.is_empty() {
            String::new()
        } else {
            format!(r#""protocolVersion":{version},"#)
        }
    };
    let mut lines = Vec::new();
    for (index, (requested, answered)) in exchanges.iter().enumerate() {
        lines.push(line(
            2 * index,
            "client-to-server",
            &format!(
                r#"{{"jsonrpc":"2.0","id":{index},"method":"initialize","params":{{{}"capabilities":{{}}}}}}"#,
                member(requested)
            ),
        ));
        lines.push(line(
            2 * index + 1,
            "server-to-client",
            &format!(
                r#"{{"jsonrpc":"2.0","id":{index},"result":{{{}"capabilities":{{}}}}}}"#,
                member(answered)
            ),
        ));
    }
    lines.join("\n")
}

/// The check's finding `(seq, detail)`s and subject count.
fn run(document: &str) -> (Vec<(Option<u64>, String)>, u32) {
    let events = parse_trace(document, &Limits::default()).expect("test trace parses");
    let context = TraceContext::new(&events);
    let outcome = checks::find(CHECK)
        .expect("check is registered")
        .run(&context);
    (
        outcome
            .findings
            .into_iter()
            .map(|found| (found.seq, found.detail))
            .collect(),
        outcome.subjects,
    )
}

fn seqs(exchanges: &[(&str, &str)]) -> (Vec<Option<u64>>, u32) {
    let (findings, subjects) = run(&trace(exchanges));
    (findings.into_iter().map(|(seq, _)| seq).collect(), subjects)
}

const UNSUPPORTED: &str = r#""1970-01-01""#;
const OLD: &str = r#""2025-03-26""#;
const NEW: &str = r#""2025-11-25""#;

#[test]
fn a_fallback_older_than_a_version_agreed_elsewhere_is_reported() {
    // In either order: the later agreement shows what the server supports.
    assert_eq!(seqs(&[(UNSUPPORTED, OLD), (NEW, NEW)]), (vec![Some(1)], 1));
    assert_eq!(seqs(&[(NEW, NEW), (UNSUPPORTED, OLD)]), (vec![Some(3)], 1));
    let (findings, _) = run(&trace(&[(UNSUPPORTED, OLD), (NEW, NEW)]));
    assert_eq!(
        findings[0].1,
        "the server answered the unsupported version \"1970-01-01\" with \"2025-03-26\", but \
         another `initialize` result in this trace names \"2025-11-25\", a later version it \
         supports"
    );
}

#[test]
fn a_version_offered_in_another_fallback_is_evidence_too() {
    // Two fallbacks, both subjects; only the older one is behind.
    assert_eq!(
        seqs(&[(UNSUPPORTED, OLD), (r#""1999-09-09""#, NEW)]),
        (vec![Some(1)], 2)
    );
}

#[test]
fn a_fallback_to_the_latest_version_seen_passes() {
    assert_eq!(seqs(&[(UNSUPPORTED, NEW), (OLD, OLD)]), (vec![], 1));
    assert_eq!(seqs(&[(UNSUPPORTED, NEW), (NEW, NEW)]), (vec![], 1));
    assert_eq!(
        seqs(&[(UNSUPPORTED, NEW), (r#""1999-09-09""#, NEW)]),
        (vec![], 2)
    );
}

#[test]
fn one_answered_initialize_has_nothing_to_be_compared_with() {
    assert_eq!(seqs(&[(UNSUPPORTED, OLD)]), (vec![], 0));
    assert_eq!(seqs(&[]), (vec![], 0));
    // A second exchange that names no dated version is not evidence: an
    // error, a missing member, a non-string, an undated string.
    for answered in ["", "7", r#""latest""#] {
        assert_eq!(
            seqs(&[(UNSUPPORTED, OLD), (NEW, answered)]),
            (vec![], 0),
            "{answered}"
        );
    }
    let mut refused = trace(&[(UNSUPPORTED, OLD)]);
    refused.push('\n');
    refused.push_str(&line(
        2,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
    ));
    refused.push('\n');
    refused.push_str(&line(
        3,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32602,"message":"Unsupported protocol version"}}"#,
    ));
    assert_eq!(run(&refused), (vec![], 0));
}

#[test]
fn an_agreement_is_not_a_fallback() {
    // Agreed versions are never subjects, however they compare.
    assert_eq!(seqs(&[(OLD, OLD), (NEW, NEW)]), (vec![], 0));
    // A request naming no string version does not say what was unsupported,
    // but its answer still shows a version the server supports.
    assert_eq!(seqs(&[("", OLD), (NEW, NEW)]), (vec![], 0));
    assert_eq!(seqs(&[("5", OLD), (NEW, NEW)]), (vec![], 0));
    assert_eq!(seqs(&[(UNSUPPORTED, OLD), ("", NEW)]), (vec![Some(1)], 1));
}

#[test]
fn only_the_clients_initialize_is_read() {
    // A server-sent `initialize` answered by the client is not a negotiation.
    let mut document = trace(&[(UNSUPPORTED, OLD)]);
    document.push('\n');
    document.push_str(&line(
        2,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":"s","method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
    ));
    document.push('\n');
    document.push_str(&line(
        3,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":"s","result":{"protocolVersion":"2025-11-25"}}"#,
    ));
    assert_eq!(run(&document), (vec![], 0));
}
