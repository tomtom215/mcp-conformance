// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` prompts requirements (`PROM-*`).
//!
//! Content-shape evidence comes from `prompts/get` results: image and audio content
//! items must carry base64 data with a MIME type, and embedded resources must carry a
//! URI, a MIME type, and exactly one of text or blob.

use serde_json::Value;

use super::FindingSink;
use super::support::{has_rfc3986_scheme, is_base64, server_capability};
use crate::context::TraceContext;

/// `PROM-001`: "Servers that support prompts MUST declare the `prompts` capability
/// during initialization:" — successfully serving prompts traffic is the observable
/// form of support.
pub(super) fn capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // The subject is an answered prompts exchange, declaration or not; a
    // session that never touched prompts leaves this clause untested.
    let declared = server_capability(context, &["prompts"]) != Some(false);
    for exchange in context.exchanges() {
        if matches!(exchange.method, "prompts/list" | "prompts/get") && exchange.result.is_some() {
            sink.examined();
            if !declared {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "server answered {:?} without declaring the prompts capability",
                        exchange.method
                    ),
                );
            }
        }
    }
}

/// The content items of every `prompts/get` result message, with their event `seq`.
fn prompt_content_items<'a>(context: &TraceContext<'a>) -> Vec<(u64, &'a Value)> {
    let mut items = Vec::new();
    for exchange in context.exchanges_for("prompts/get") {
        let messages = exchange
            .result
            .and_then(|result| result.get("messages"))
            .and_then(Value::as_array);
        for message in messages.into_iter().flatten() {
            if let Some(content) = message.get("content") {
                items.push((exchange.response.seq, content));
            }
        }
    }
    items
}

/// The required-argument names each `prompts/list` result declared, by prompt name.
///
/// The server's own declaration is the ground truth this check needs, and it is in
/// the trace — which is the whole reason `PROM-008` is judgeable at all.
fn declared_required_arguments<'a>(
    context: &TraceContext<'a>,
) -> std::collections::BTreeMap<&'a str, Vec<&'a str>> {
    let mut declared = std::collections::BTreeMap::new();
    for exchange in context.exchanges_for("prompts/list") {
        let prompts = exchange
            .result
            .and_then(|result| result.get("prompts"))
            .and_then(Value::as_array);
        for prompt in prompts.into_iter().flatten() {
            let Some(name) = prompt.get("name").and_then(Value::as_str) else {
                continue;
            };
            let required: Vec<&str> = prompt
                .get("arguments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|argument| argument.get("required").and_then(Value::as_bool) == Some(true))
                .filter_map(|argument| argument.get("name").and_then(Value::as_str))
                .collect();
            if !required.is_empty() {
                // A later listing wins: the server may re-declare its surface.
                declared.insert(name, required);
            }
        }
    }
    declared
}

/// `PROM-008`: "Servers SHOULD validate prompt arguments before processing".
///
/// Validation *thoroughness* is internal, but validation *failure* is not: a server
/// that answers a `prompts/get` with a successful result, when that request omits an
/// argument the server itself published as `required` in `prompts/list`, demonstrably
/// processed input it had declared invalid. Both halves of that comparison — the
/// declaration and the call — are recorded messages, so no server-side knowledge is
/// needed. This is the same cross-message correlation `LIFE-009` uses for capability
/// gating and `PAGE-002` for cursors.
///
/// Deliberately narrow, because a conformance verdict is only worth its soundness:
/// - It fires only on prompts whose required arguments were *observed* being declared;
///   an unlisted prompt, or a session with no `prompts/list`, is not judged.
/// - It fires only when the server returned a **result**. An error response means the
///   server did reject the call, which satisfies this clause; whether that error
///   carries exactly `-32602` is `PROM-007`'s enumeration, and that requirement stays
///   excluded because its other two cases (invalid name, internal error) remain
///   server-side ground truth.
pub(super) fn arguments_validated(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let declared = declared_required_arguments(context);
    if declared.is_empty() {
        return;
    }
    for exchange in context.exchanges_for("prompts/get") {
        if exchange.result.is_none() {
            continue;
        }
        let Some(params) = exchange.params else {
            continue;
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(required) = declared.get(name) else {
            continue;
        };
        sink.examined();
        let supplied = params.get("arguments");
        let missing: Vec<&str> = required
            .iter()
            .filter(|argument| {
                supplied
                    .and_then(|arguments| arguments.get(*argument))
                    .is_none()
            })
            .copied()
            .collect();
        if missing.is_empty() {
            continue;
        }
        sink.push(
            Some(exchange.response.seq),
            format!(
                "server returned a result for prompts/get {name:?} although the request \
                 omitted the required argument(s) {missing:?} it declared in prompts/list"
            ),
        );
    }
}

/// `PROM-003`: image content data must be base64 with a MIME type present.
pub(super) fn image_content_encoding(context: &TraceContext<'_>, sink: &mut FindingSink) {
    binary_content_encoding(context, sink, "image");
}

/// `PROM-004`: audio content data must be base64 with a MIME type present.
pub(super) fn audio_content_encoding(context: &TraceContext<'_>, sink: &mut FindingSink) {
    binary_content_encoding(context, sink, "audio");
}

fn binary_content_encoding(context: &TraceContext<'_>, sink: &mut FindingSink, kind: &str) {
    for (seq, content) in prompt_content_items(context) {
        if content.get("type").and_then(Value::as_str) != Some(kind) {
            continue;
        }
        sink.examined();
        let data_valid = content
            .get("data")
            .and_then(Value::as_str)
            .is_some_and(is_base64);
        if !data_valid {
            sink.push(
                Some(seq),
                format!("{kind} content data is not valid base64"),
            );
        }
        let mime_present = content
            .get("mimeType")
            .and_then(Value::as_str)
            .is_some_and(|mime| {
                mime.split_once('/')
                    .is_some_and(|(t, s)| !t.is_empty() && !s.is_empty())
            });
        if !mime_present {
            sink.push(
                Some(seq),
                format!("{kind} content lacks a valid mimeType (expected type/subtype)"),
            );
        }
    }
}

/// `PROM-005`: embedded resources must include a valid resource URI, the appropriate
/// MIME type, and either text or base64 blob data.
pub(super) fn embedded_resource_shape(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, content) in prompt_content_items(context) {
        if content.get("type").and_then(Value::as_str) != Some("resource") {
            continue;
        }
        sink.examined();
        let Some(resource) = content.get("resource") else {
            sink.push(
                Some(seq),
                "embedded resource content lacks the resource member".to_owned(),
            );
            continue;
        };
        let uri_ok = resource
            .get("uri")
            .and_then(Value::as_str)
            .is_some_and(has_rfc3986_scheme);
        if !uri_ok {
            sink.push(
                Some(seq),
                "embedded resource lacks a valid resource URI".to_owned(),
            );
        }
        if resource.get("mimeType").and_then(Value::as_str).is_none() {
            sink.push(Some(seq), "embedded resource lacks a mimeType".to_owned());
        }
        let text = resource.get("text").and_then(Value::as_str);
        let blob = resource.get("blob").and_then(Value::as_str);
        match (text, blob) {
            (Some(_), None) => {}
            (None, Some(blob)) if is_base64(blob) => {}
            (None, Some(_)) => sink.push(
                Some(seq),
                "embedded resource blob is not valid base64".to_owned(),
            ),
            (Some(_), Some(_)) => sink.push(
                Some(seq),
                "embedded resource carries both text and blob; expected exactly one".to_owned(),
            ),
            (None, None) => sink.push(
                Some(seq),
                "embedded resource carries neither text nor blob data".to_owned(),
            ),
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
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    const HANDSHAKE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"prompts":{}},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;

    fn get_prompt_with_content(content: &str) -> String {
        let request = r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"p"}}}"#;
        let result = format!(
            r#"{{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":2,"result":{{"messages":[{{"role":"user","content":{content}}}]}}}}}}"#
        );
        format!("{HANDSHAKE}\n{request}\n{result}")
    }

    /// Builds a session that lists `review` with a required `diff` argument, then
    /// issues `prompts/get` with `arguments` and gets `response` back.
    fn listed_then_called(arguments: &str, response: &str) -> String {
        let list = r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"prompts/list"}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"prompts":[{"name":"review","arguments":[{"name":"diff","required":true},{"name":"tone","required":false}]}]}}}"#;
        let get = format!(
            r#"{{"seq":5,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":3,"method":"prompts/get","params":{{"name":"review","arguments":{arguments}}}}}}}"#
        );
        format!("{HANDSHAKE}\n{list}\n{get}\n{response}")
    }

    const RESULT: &str = r#"{"seq":6,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"result":{"messages":[]}}}"#;
    const ERROR: &str = r#"{"seq":6,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"missing required argument: diff"}}}"#;

    #[test]
    fn serving_a_result_despite_a_missing_required_argument_is_a_finding() {
        let trace = listed_then_called(r#"{"tone":"terse"}"#, RESULT);
        let findings = findings_for("prompts.arguments-validated", &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("\"diff\""), "{findings:?}");
        assert!(findings[0].contains("review"), "{findings:?}");
    }

    #[test]
    fn supplying_the_required_argument_is_clean() {
        let trace = listed_then_called(r#"{"diff":"--- a\n+++ b"}"#, RESULT);
        assert!(findings_for("prompts.arguments-validated", &trace).is_empty());
    }

    #[test]
    fn an_omitted_optional_argument_is_not_a_finding() {
        // `tone` is declared `required: false`; only `diff` may be demanded.
        let trace = listed_then_called(r#"{"diff":"d"}"#, RESULT);
        assert!(findings_for("prompts.arguments-validated", &trace).is_empty());
    }

    #[test]
    fn rejecting_the_call_satisfies_the_clause() {
        // The server *did* validate — an error response is the compliant answer,
        // so the check must stay silent even though the argument was missing.
        let trace = listed_then_called(r#"{"tone":"terse"}"#, ERROR);
        assert!(findings_for("prompts.arguments-validated", &trace).is_empty());
    }

    #[test]
    fn a_prompt_never_listed_is_not_judged() {
        // Without an observed declaration there is no ground truth, so a
        // `prompts/get` for an unlisted prompt is outside the check's reach.
        let get = r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"unlisted"}}}"#;
        let result = r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"messages":[]}}}"#;
        let trace = format!("{HANDSHAKE}\n{get}\n{result}");
        assert!(findings_for("prompts.arguments-validated", &trace).is_empty());
    }

    #[test]
    fn a_missing_arguments_object_entirely_is_still_a_finding() {
        // Omitting `arguments` altogether must read the same as omitting the
        // required key inside it.
        let get = r#"{"seq":5,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"method":"prompts/get","params":{"name":"review"}}}"#;
        let list = r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"prompts/list"}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"prompts":[{"name":"review","arguments":[{"name":"diff","required":true}]}]}}}"#;
        let trace = format!("{HANDSHAKE}\n{list}\n{get}\n{RESULT}");
        assert_eq!(findings_for("prompts.arguments-validated", &trace).len(), 1);
    }

    #[test]
    fn image_and_audio_checks_are_type_scoped() {
        // A bad *audio* item must not produce *image* findings, and vice versa.
        let trace = get_prompt_with_content(
            r#"{"type":"audio","data":"not base64!","mimeType":"audio/wav"}"#,
        );
        assert!(findings_for("prompts.image-content-encoding", &trace).is_empty());
        let findings = findings_for("prompts.audio-content-encoding", &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("audio content data"), "{findings:?}");
    }

    #[test]
    fn mime_type_must_be_type_slash_subtype() {
        for (mime, expect_finding) in [
            (r#""image/png""#, false),
            (r#""image/""#, true),
            (r#""png""#, true),
            ("42", true),
        ] {
            let trace = get_prompt_with_content(&format!(
                r#"{{"type":"image","data":"QUJDRA==","mimeType":{mime}}}"#
            ));
            let findings = findings_for("prompts.image-content-encoding", &trace);
            assert_eq!(!findings.is_empty(), expect_finding, "{mime}: {findings:?}");
        }
    }

    #[test]
    fn embedded_resource_shape_flags_each_defect_once() {
        let trace = get_prompt_with_content(
            r#"{"type":"resource","resource":{"uri":"no scheme","text":"x","blob":"QUJDRA=="}}"#,
        );
        let findings = findings_for("prompts.embedded-resource-shape", &trace);
        // Bad URI, missing mimeType, and text+blob together: three findings.
        assert_eq!(findings.len(), 3, "{findings:?}");
    }

    #[test]
    fn well_formed_embedded_resource_passes() {
        let trace = get_prompt_with_content(
            r#"{"type":"resource","resource":{"uri":"file:///a.txt","mimeType":"text/plain","text":"hello"}}"#,
        );
        assert!(findings_for("prompts.embedded-resource-shape", &trace).is_empty());
    }

    #[test]
    fn blob_only_embedded_resources_hinge_on_base64_validity() {
        // Valid blob, no text: well-formed, zero findings.
        let valid = get_prompt_with_content(
            r#"{"type":"resource","resource":{"uri":"file:///a.png","mimeType":"image/png","blob":"QUJDRA=="}}"#,
        );
        assert!(findings_for("prompts.embedded-resource-shape", &valid).is_empty());

        // Invalid blob, no text: exactly the base64 finding.
        let invalid = get_prompt_with_content(
            r#"{"type":"resource","resource":{"uri":"file:///a.png","mimeType":"image/png","blob":"not base64!"}}"#,
        );
        let findings = findings_for("prompts.embedded-resource-shape", &invalid);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("not valid base64"), "{findings:?}");
    }
}
