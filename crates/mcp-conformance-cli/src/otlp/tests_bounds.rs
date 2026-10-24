// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a span is *not* sent, where the conventions' field is free text
//! from the session: the server's error message, a resource URI past its
//! path, a finding's detail past [`DETAIL_LIMIT`].

#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::tests::{SPAN, TRACE, at, attribute, encoded, exchange, exchange_with, finding};
use super::*;
use crate::live::Ending;

/// The server's error message is free text — it can quote a payload, a path
/// or a secret — so the span's status never carries it: only the numeric
/// code and, for a code JSON-RPC 2.0 or MCP defines, that code's name.
#[test]
fn the_error_status_never_carries_the_servers_message() {
    let mut failed = exchange("tools/call", Some("1"));
    for (code, descriptor) in [
        ("-32700", "JSON-RPC error -32700 (parse error)"),
        ("-32600", "JSON-RPC error -32600 (invalid request)"),
        ("-32601", "JSON-RPC error -32601 (method not found)"),
        ("-32602", "JSON-RPC error -32602 (invalid params)"),
        ("-32603", "JSON-RPC error -32603 (internal error)"),
        ("-32002", "JSON-RPC error -32002 (resource not found)"),
        ("-32000", "JSON-RPC error -32000"),
        ("7", "JSON-RPC error 7"),
    ] {
        failed.ending = Ending::Answered {
            error: Some((
                Some(code.to_owned()),
                Some("token=SECRET-MESSAGE for /home/me".to_owned()),
            )),
            tool_error: false,
        };
        let span = encoded(&span(&failed, TRACE, SPAN, None));
        assert_eq!(span["status"]["message"], descriptor, "{code}");
        assert!(
            !span.to_string().contains("SECRET-MESSAGE"),
            "{code}: {span}"
        );
    }
}

/// A resource URI is a required attribute of the conventions, but a
/// presigned or tokenised one carries its credential in the query (or the
/// userinfo): what is exported is the URI up to its path.
#[test]
fn a_resource_uri_is_exported_without_userinfo_query_or_fragment() {
    for (uri, exported) in [
        ("file:///r", "file:///r"),
        (
            "https://files.example/doc?X-Amz-Signature=SECRET-Q#SECRET-F",
            "https://files.example/doc",
        ),
        (
            "https://u:SECRET-P@h.example:8443/x",
            "https://h.example:8443/x",
        ),
        ("s3://key:SECRET-S@bucket/object", "s3://bucket/object"),
        ("urn:isbn:0451450523?x=SECRET-U", "urn:isbn:0451450523"),
        (
            "custom://host/path@not@userinfo",
            "custom://host/path@not@userinfo",
        ),
        ("custom://u@host?q", "custom://host"),
        ("?SECRET-ONLY", ""),
    ] {
        assert_eq!(exported_uri(uri), exported, "{uri}");
        let params = format!(r#"{{"uri":{}}}"#, json!(uri));
        let read = encoded(&span(
            &exchange_with("resources/read", Some("3"), &params),
            TRACE,
            SPAN,
            None,
        ));
        assert_eq!(
            attribute(&read, "mcp.resource.uri"),
            Some(json!({"stringValue": exported})),
            "{uri}"
        );
        assert!(!read.to_string().contains("SECRET"), "{uri}: {read}");
    }
}

/// A finding's detail quotes what it is about — a tool name, say — and a
/// hostile one can be as long as a message: it is cut at [`DETAIL_LIMIT`]
/// characters, and the cut is marked.
#[test]
fn a_findings_detail_is_cut_at_the_limit_and_the_cut_is_marked() {
    let detail_of = |detail: String| {
        let mut span = span(&exchange("tools/list", Some("2")), TRACE, SPAN, None);
        let mut found = finding(Some(4));
        found.detail = detail;
        span.add_finding(&found, at(0));
        attribute(&encoded(&span)["events"][0], "mcp_conformance.detail").unwrap()["stringValue"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let exact = "é".repeat(DETAIL_LIMIT);
    assert_eq!(detail_of(exact.clone()), exact, "at the limit: unchanged");
    let long = format!("{}SECRET-TAIL", "é".repeat(DETAIL_LIMIT));
    let cut = detail_of(long);
    assert!(cut.starts_with(&exact), "{cut}");
    assert!(!cut.contains("SECRET-TAIL"), "{cut}");
    assert!(
        cut.ends_with(&format!(
            "… [cut at {DETAIL_LIMIT} of {} chars]",
            DETAIL_LIMIT + "SECRET-TAIL".len()
        )),
        "{cut}"
    );
}
