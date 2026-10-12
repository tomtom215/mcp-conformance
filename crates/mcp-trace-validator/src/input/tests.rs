// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the hints that follow a trace the reader refused.
//!
//! Each hint fires on one shape of document and stays quiet on its
//! neighbours: a hint on the wrong document misdiagnoses it, which is worse
//! than no hint at all.

#![allow(clippy::unwrap_used)]

use mcp_trace_validator::reader::{Limits, parse_trace};

use super::{Refusal, parse_hint};

const EVENT: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#;

/// The hint the reader's error on `document` earns.
fn hint(document: &str) -> Option<String> {
    let error = parse_trace(document, &Limits::default()).unwrap_err();
    parse_hint(&error, document)
}

#[test]
fn a_blank_line_is_called_trailing_only_when_nothing_follows_it() {
    let trailing = hint(&format!("{EVENT}\n\n")).unwrap();
    assert!(
        trailing.contains("a trailing blank line at the end of the file"),
        "{trailing}"
    );
    // Several blank lines at the end are still the end of the file.
    assert!(hint(&format!("{EVENT}\n\n \n")).is_some());
    // A blank line with a record after it is an interior one.
    assert_eq!(hint(&format!("{EVENT}\n\n{EVENT}\n")), None);
}

#[test]
fn a_bare_json_rpc_message_is_told_to_be_recorded_as_a_trace() {
    let bare = hint(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap();
    assert!(bare.contains("bare JSON-RPC message"), "{bare}");
    assert!(bare.contains("mcp-trace-capture"), "{bare}");
    // On a later line too: the line the error names is the one examined.
    let later = format!("{EVENT}\n{}", r#"{"jsonrpc":"2.0","method":"ping"}"#);
    assert!(hint(&later).is_some());
    // An event with a broken envelope is not a bare message…
    assert_eq!(
        hint(r#"{"jsonrpc":"2.0","kind":"message","direction":"client-to-server"}"#),
        None
    );
    // …and neither is an object that is not JSON-RPC at all.
    assert_eq!(hint(r#"{"method":"ping"}"#), None);
}

#[test]
fn the_limits_name_the_flag_that_lifts_them() {
    let error = parse_trace(EVENT, &Limits::new(10, 4)).unwrap_err();
    let line = parse_hint(&error, EVENT).unwrap();
    assert!(line.contains("--max-line-bytes 104"), "{line}");
    let error = parse_trace(&format!("{EVENT}\n{EVENT}"), &Limits::new(1, 1024)).unwrap_err();
    assert!(
        parse_hint(&error, EVENT)
            .unwrap()
            .contains("--max-events above 1")
    );
}

#[test]
fn a_diagnostic_loses_only_its_terminal_prefix() {
    let refusal = Refusal::from_diagnostic("error: a.jsonl judged nothing", 2);
    assert_eq!(refusal.error, "a.jsonl judged nothing");
    assert_eq!(refusal.code, 2);
    assert_eq!(
        Refusal::from_diagnostic("a.jsonl judged nothing", 2).error,
        "a.jsonl judged nothing"
    );
}
