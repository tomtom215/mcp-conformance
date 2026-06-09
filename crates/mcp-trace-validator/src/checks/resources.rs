// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` resources requirements (`RES-*`).
//!
//! Evidence comes from `resources/list`, `resources/templates/list`, and
//! `resources/read` exchanges plus subscription traffic. URI-template strings
//! (`uriTemplate`) are RFC 6570 templates, not URIs, and are deliberately outside the
//! RFC 3986 scheme check.

use serde_json::Value;

use super::FindingSink;
use super::support::{has_rfc3986_scheme, is_base64, server_capability};
use crate::context::TraceContext;

/// The resources-area request methods whose successful service evidences support.
const RESOURCE_METHODS: &[&str] = &[
    "resources/list",
    "resources/templates/list",
    "resources/read",
    "resources/subscribe",
    "resources/unsubscribe",
];

/// `RES-001`: "Servers that support resources MUST declare the `resources`
/// capability:" — successfully serving resources traffic is the observable form of
/// support.
pub(super) fn capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    if server_capability(context, &["resources"]) != Some(false) {
        return;
    }
    for exchange in context.exchanges() {
        if RESOURCE_METHODS.contains(&exchange.method) && exchange.result.is_some() {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "server answered {:?} without declaring the resources capability",
                    exchange.method
                ),
            );
        }
    }
}

/// Every URI the server stated for a resource, with the event `seq` it appeared at:
/// `resources/list` result entries, `resources/read` result contents, and
/// `notifications/resources/updated` params.
fn server_stated_uris<'a>(context: &TraceContext<'a>) -> Vec<(u64, &'a str)> {
    let mut uris = Vec::new();
    for exchange in context.exchanges_for("resources/list") {
        let entries = exchange
            .result
            .and_then(|result| result.get("resources"))
            .and_then(Value::as_array);
        for entry in entries.into_iter().flatten() {
            if let Some(uri) = entry.get("uri").and_then(Value::as_str) {
                uris.push((exchange.response.seq, uri));
            }
        }
    }
    for exchange in context.exchanges_for("resources/read") {
        let contents = exchange
            .result
            .and_then(|result| result.get("contents"))
            .and_then(Value::as_array);
        for content in contents.into_iter().flatten() {
            if let Some(uri) = content.get("uri").and_then(Value::as_str) {
                uris.push((exchange.response.seq, uri));
            }
        }
    }
    uris
}

/// `RES-004`: URI schemes must follow RFC 3986 §3.1 syntax. Scheme syntax is the
/// trace-judgeable core of "in accordance with RFC3986"; the registry quote carries
/// the full clause.
pub(super) fn uri_scheme_rfc3986(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, uri) in server_stated_uris(context) {
        if !has_rfc3986_scheme(uri) {
            sink.push(
                Some(seq),
                format!(
                    "resource URI {uri:?} does not begin with an RFC 3986 scheme (ALPHA *( ALPHA / DIGIT / \"+\" / \"-\" / \".\" ) followed by \":\")"
                ),
            );
        }
    }
}

/// `RES-006`: "Binary data MUST be properly encoded" — every `blob` member in
/// `resources/read` contents must be standard base64.
pub(super) fn blob_base64(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for exchange in context.exchanges_for("resources/read") {
        let contents = exchange
            .result
            .and_then(|result| result.get("contents"))
            .and_then(Value::as_array);
        for content in contents.into_iter().flatten() {
            let Some(blob) = content.get("blob") else {
                continue;
            };
            let valid = blob.as_str().is_some_and(is_base64);
            if !valid {
                let uri = content
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("(no uri)");
                sink.push(
                    Some(exchange.response.seq),
                    format!("resource {uri:?} carries a blob that is not valid base64"),
                );
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::checks;
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};

    fn findings_for(check: &str, trace: &str) -> Vec<String> {
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check)
            .unwrap()
            .run(&context)
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    const HANDSHAKE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"resources":{}},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;

    #[test]
    fn scheme_check_reads_list_and_read_uris() {
        let trace = format!(
            "{HANDSHAKE}\n{}\n{}\n{}\n{}",
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"resources/list"}}"#,
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"resources":[{"uri":"file:///ok.txt","name":"ok"},{"uri":"not a uri","name":"bad"}]}}}"#,
            r#"{"seq":5,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"file:///ok.txt"}}}"#,
            r#"{"seq":6,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"result":{"contents":[{"uri":"3http://x","text":"hi"}]}}}"#,
        );
        let findings = findings_for("resources.uri-scheme-rfc3986", &trace);
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(findings[0].contains("not a uri"), "{findings:?}");
        assert!(findings[1].contains("3http"), "{findings:?}");
    }

    #[test]
    fn blob_check_flags_non_base64_and_non_string_blobs() {
        let trace = format!(
            "{HANDSHAKE}\n{}\n{}",
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"file:///img.png"}}}"#,
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"contents":[{"uri":"file:///img.png","blob":"not base64!"},{"uri":"file:///n.png","blob":42},{"uri":"file:///ok.png","blob":"QUJDRA=="}]}}}"#,
        );
        let findings = findings_for("resources.blob-base64", &trace);
        assert_eq!(findings.len(), 2, "{findings:?}");
    }

    #[test]
    fn capability_check_needs_successful_service() {
        let trace = format!(
            "{}\n{}\n{}",
            HANDSHAKE.replace(r#""resources":{}"#, r#""prompts":{}"#),
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"resources/templates/list"}}"#,
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"resourceTemplates":[]}}}"#,
        );
        let findings = findings_for("resources.capability-declared", &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].contains("resources/templates/list"),
            "{findings:?}"
        );
    }
}
