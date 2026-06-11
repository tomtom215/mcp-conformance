// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the base JSON-RPC message requirements (`BASE-*`).
//!
//! These operate per message and rely on [`classify`]'s leniency: malformed messages
//! are reported precisely rather than aborting the run.

use std::collections::HashMap;

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::message::{MessageKind, is_notification_method};
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::FindingSink;
use crate::context::TraceContext;

mod meta;

pub(super) use meta::meta_key_format;

/// Human name of a JSON value's type, for finding details.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "an integer"
            } else {
                "a non-integer number"
            }
        }
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn id_is_string_or_integer(id: &Value) -> bool {
    match id {
        Value::String(_) => true,
        Value::Number(number) => number.is_i64() || number.is_u64(),
        _ => false,
    }
}

/// `BASE-001`: "Requests MUST include a string or integer ID."
pub(super) fn request_id_type(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if let MessageKind::Request { method, id } = kind
            && !id_is_string_or_integer(id)
        {
            sink.push(
                    Some(event.seq),
                    format!(
                        "request {method:?} carries {} as its id; the ID must be a string or an integer",
                        type_name(id)
                    ),
                );
        }
    }
}

/// `BASE-002`: "Unlike base JSON-RPC, the ID MUST NOT be `null`."
pub(super) fn request_id_not_null(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if let MessageKind::Request { method, id } = kind
            && id.is_null()
        {
            sink.push(
                Some(event.seq),
                format!("request {method:?} carries a null id, which MCP forbids"),
            );
        }
    }
}

/// `BASE-003`: "The request ID MUST NOT have been previously used by the requestor
/// within the same session."
pub(super) fn request_id_unique(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut first_use: HashMap<(Direction, String), u64> = HashMap::new();
    for (event, kind, _) in context.messages() {
        if let MessageKind::Request { method, id } = kind {
            if id.is_null() {
                continue; // BASE-002's finding; don't double-report.
            }
            let key = (event.direction, to_canonical_string(id));
            match first_use.get(&key) {
                Some(previous) => sink.push(
                    Some(event.seq),
                    format!(
                        "request {method:?} reuses id {}, already used by the same party at seq {previous}",
                        key.1
                    ),
                ),
                None => {
                    first_use.insert(key, event.seq);
                }
            }
        }
    }
}

/// Walks responses and reports those of the wanted flavor that match no outstanding
/// request from the opposite party. Shared by `BASE-004` (results) and `BASE-009`
/// (errors).
///
/// A request is answered exactly once, by a result XOR an error. Each flavor's pass
/// therefore consumes the outstanding entry on *both* flavors — its own (flagging a
/// mismatch) and the other's (silently, as that other response is the legitimate
/// first answer). The consequence: a request answered by both a result and an error,
/// in either order, leaves the *second* response with no outstanding request, and the
/// pass for the second response's flavor flags it. Without the cross-flavor consume,
/// each pass saw a clean 1-request→1-response and a double-answer slipped through.
fn responses_match_requests(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    want_results: bool,
) {
    // Outstanding request ids per requesting party, canonical id -> request seq.
    let mut outstanding: HashMap<(Direction, String), u64> = HashMap::new();
    for (event, kind, _) in context.messages() {
        match kind {
            MessageKind::Request { id, .. } => {
                if !id.is_null() {
                    outstanding.insert((event.direction, to_canonical_string(id)), event.seq);
                }
            }
            MessageKind::Result { id } => {
                if want_results {
                    check_response_id(
                        event.seq,
                        event.direction,
                        *id,
                        &mut outstanding,
                        sink,
                        "result",
                    );
                } else {
                    // The other flavor's valid first answer: consume so a later
                    // same-id error is seen as answering an already-answered request.
                    consume_outstanding(event.direction, *id, &mut outstanding);
                }
            }
            MessageKind::Error { id, .. } => {
                // The null/absent-id condition is the spec's escape hatch ("except in
                // error cases where the ID could not be read due a malformed request"),
                // so a null/absent error id is neither flagged nor consumes anything.
                if want_results {
                    consume_outstanding(event.direction, *id, &mut outstanding);
                } else if id.is_some_and(|id| !id.is_null()) {
                    check_response_id(
                        event.seq,
                        event.direction,
                        *id,
                        &mut outstanding,
                        sink,
                        "error",
                    );
                }
            }
            _ => {}
        }
    }
}

/// Removes the outstanding request a response answers, without flagging — the path
/// for a response of the flavor a given pass does not judge. A null/absent id matches
/// no request and removes nothing.
fn consume_outstanding(
    response_direction: Direction,
    id: Option<&Value>,
    outstanding: &mut HashMap<(Direction, String), u64>,
) {
    if let Some(id) = id.filter(|id| !id.is_null()) {
        let requester = match response_direction {
            Direction::ClientToServer => Direction::ServerToClient,
            Direction::ServerToClient => Direction::ClientToServer,
        };
        outstanding.remove(&(requester, to_canonical_string(id)));
    }
}

fn check_response_id(
    seq: u64,
    response_direction: Direction,
    id: Option<&Value>,
    outstanding: &mut HashMap<(Direction, String), u64>,
    sink: &mut FindingSink,
    flavor: &str,
) {
    let requester = match response_direction {
        Direction::ClientToServer => Direction::ServerToClient,
        Direction::ServerToClient => Direction::ClientToServer,
    };
    match id {
        None => sink.push(
            Some(seq),
            format!("{flavor} response is missing its id; responses must echo the request id"),
        ),
        Some(id) if id.is_null() => sink.push(
            Some(seq),
            format!("{flavor} response carries a null id; responses must echo the request id"),
        ),
        Some(id) => {
            let key = (requester, to_canonical_string(id));
            if outstanding.remove(&key).is_none() {
                sink.push(
                    Some(seq),
                    format!(
                        "{flavor} response answers id {}, but that party has no outstanding request with that id (never sent, or already answered)",
                        key.1
                    ),
                );
            }
        }
    }
}

/// `BASE-004`: "Result responses MUST include the same ID as the request they
/// correspond to."
pub(super) fn result_id_matches(context: &TraceContext<'_>, sink: &mut FindingSink) {
    responses_match_requests(context, sink, true);
}

/// `BASE-009`: "Error responses MUST include the same ID as the request they correspond
/// to (except in error cases where the ID could not be read due a malformed request)."
pub(super) fn error_id_matches(context: &TraceContext<'_>, sink: &mut FindingSink) {
    responses_match_requests(context, sink, false);
}

/// `BASE-005`: "Notifications MUST NOT include an ID."
///
/// A message in the reserved `notifications/` namespace that carries an `id`
/// classifies structurally as a request; this check is what catches it.
pub(super) fn notification_no_id(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if let MessageKind::Request { method, .. } = kind
            && is_notification_method(method)
        {
            sink.push(
                    Some(event.seq),
                    format!(
                        "{method:?} is a notification method but the message carries an id; notifications must not include one"
                    ),
                );
        }
    }
}

/// `BASE-006`: "Error responses MUST include an `error` field with a `code` and
/// `message`."
pub(super) fn error_shape(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if let MessageKind::Error { error, .. } = kind {
            let Some(object) = error.as_object() else {
                sink.push(
                    Some(event.seq),
                    format!("error member is {}, expected an object", type_name(error)),
                );
                continue;
            };
            if !object.contains_key("code") {
                sink.push(
                    Some(event.seq),
                    "error object lacks a code member".to_owned(),
                );
            }
            match object.get("message") {
                None => sink.push(
                    Some(event.seq),
                    "error object lacks a message member".to_owned(),
                ),
                Some(message) if !message.is_string() => sink.push(
                    Some(event.seq),
                    format!(
                        "error message member is {}, expected a string",
                        type_name(message)
                    ),
                ),
                Some(_) => {}
            }
        }
    }
}

/// `BASE-007`: "Error codes MUST be integers."
pub(super) fn error_code_integer(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if let MessageKind::Error { error, .. } = kind
            && let Some(code) = error.get("code")
            && !code.is_i64()
            && !code.is_u64()
        {
            sink.push(
                Some(event.seq),
                format!("error code is {}, expected an integer", type_name(code)),
            );
        }
    }
}

/// `BASE-010`: "Result responses MUST include a `result` field." A message carrying
/// an `id` and no `method` is response-shaped; if it then carries neither `result`
/// nor `error`, it is a result response missing its `result` member (an error
/// response would carry `error` instead).
pub(super) fn result_field(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if !matches!(kind, MessageKind::Invalid { .. }) {
            continue;
        }
        let Some(object) = event.message_payload().and_then(Value::as_object) else {
            continue;
        };
        if object.contains_key("id")
            && !object.contains_key("method")
            && !object.contains_key("result")
            && !object.contains_key("error")
        {
            sink.push(
                Some(event.seq),
                "response-shaped message (id present, no method) carries no result field"
                    .to_owned(),
            );
        }
    }
}

/// `BASE-008`: "All messages between MCP clients and servers MUST follow the JSON-RPC
/// 2.0 specification." — verified here as: the message classifies as a JSON-RPC shape
/// and carries `"jsonrpc": "2.0"`.
pub(super) fn jsonrpc_version(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if let MessageKind::Invalid { reason } = kind {
            sink.push(
                Some(event.seq),
                format!("message is not a JSON-RPC request, notification, or response: {reason}"),
            );
            continue;
        }
        let version = event
            .message_payload()
            .and_then(|payload| payload.get("jsonrpc"));
        match version {
            Some(Value::String(version)) if version == "2.0" => {}
            Some(other) => sink.push(
                Some(event.seq),
                format!("jsonrpc member is {other}, expected the string \"2.0\""),
            ),
            None => sink.push(
                Some(event.seq),
                "message lacks the jsonrpc member; JSON-RPC 2.0 requires \"jsonrpc\": \"2.0\""
                    .to_owned(),
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
    use crate::report::Finding;
    use mcp_conformance_core::trace::TraceEvent;

    fn run_check(check_id: &str, trace: &str) -> Vec<Finding> {
        let events: Vec<TraceEvent> = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check_id).unwrap().run(&context)
    }

    const INIT: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#;

    #[test]
    fn result_response_with_null_id_gets_the_null_detail() {
        // A null-id result is its own finding, distinct from "no outstanding request".
        let trace = format!(
            "{INIT}\n{}",
            r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":null,"result":{}}}"#
        );
        let findings = run_check("base.result-id-matches", &trace);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].detail.contains("null id"),
            "{}",
            findings[0].detail
        );
    }

    #[test]
    fn error_message_member_type_is_named_precisely() {
        // -5 is i64-but-not-u64: the finding must call it an integer, which pins the
        // is_i64 || is_u64 disjunction in type_name.
        let trace = format!(
            "{INIT}\n{}",
            r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":-5}}}"#
        );
        let findings = run_check("base.error-shape", &trace);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .detail
                .contains("is an integer, expected a string"),
            "{}",
            findings[0].detail
        );
    }

    #[test]
    fn u64_only_request_ids_are_valid_integers() {
        // u64::MAX is not representable as i64; it must still count as an integer id.
        let trace = format!(
            "{INIT}\n{}",
            r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":18446744073709551615,"method":"tools/list"}}"#
        );
        assert!(run_check("base.request-id-type", &trace).is_empty());
    }

    /// A request id=2 answered by both an error and a result. The SECOND answer
    /// has no outstanding request and must be flagged by its own flavor's check;
    /// the cross-flavor consume is what makes that true (without it both checks
    /// saw a clean 1:1 and the double-answer slipped through).
    const REQUEST: &str = r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#;
    const RESULT_2: &str = r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{}}}"#;
    const ERROR_2: &str = r#"{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"x"}}}"#;

    #[test]
    fn error_then_result_flags_the_second_answer_as_a_result() {
        // error (seq2) then result (seq3): the result is the double-answer, so
        // BASE-004 flags it and BASE-009 stays silent (the error was valid).
        let trace = format!("{INIT}\n{REQUEST}\n{ERROR_2}\n{RESULT_2}");
        let results = run_check("base.result-id-matches", &trace);
        assert_eq!(results.len(), 1, "{results:?}");
        assert_eq!(results[0].seq, Some(3));
        assert!(
            results[0].detail.contains("already answered"),
            "{results:?}"
        );
        assert!(
            run_check("base.error-id-matches", &trace).is_empty(),
            "the error was the legitimate first answer"
        );
    }

    #[test]
    fn result_then_error_flags_the_second_answer_as_an_error() {
        // Reverse order, so the fix cannot be order-specific: result (seq2) then
        // error (seq3) makes the error the double-answer.
        let result_seq2 = RESULT_2.replace("\"seq\":3", "\"seq\":2");
        let error_seq3 = ERROR_2.replace("\"seq\":2", "\"seq\":3");
        let trace = format!("{INIT}\n{REQUEST}\n{result_seq2}\n{error_seq3}");
        let errors = run_check("base.error-id-matches", &trace);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].seq, Some(3));
        assert!(errors[0].detail.contains("already answered"), "{errors:?}");
        assert!(
            run_check("base.result-id-matches", &trace).is_empty(),
            "the result was the legitimate first answer"
        );
    }

    #[test]
    fn single_flavor_answer_is_not_flagged_by_the_other_pass() {
        // Guard against a cross-flavor consume that over-fires: a request
        // answered once by a result must leave BOTH passes clean.
        let trace = format!(
            "{INIT}\n{REQUEST}\n{}",
            RESULT_2.replace("\"seq\":3", "\"seq\":2")
        );
        assert!(run_check("base.result-id-matches", &trace).is_empty());
        assert!(run_check("base.error-id-matches", &trace).is_empty());
    }
}
