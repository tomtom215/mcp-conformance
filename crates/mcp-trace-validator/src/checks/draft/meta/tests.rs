// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `_meta`-envelope clauses.
//!
//! The W3C `traceparent` grammar is the dense part: four fixed-width fields with
//! two all-zero prohibitions, and a corpus trace can only ever break one of
//! them. Each field is pinned here at both its length and its alphabet.

use super::validate_traceparent;
use crate::checks::draft::testkit::{META, client, findings_for, post, server, status, trace};
use serde_json::json;

/// The W3C Trace Context specification's own example value.
const VALID: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

/// A request whose `_meta` omits `clientCapabilities`, answered by `answer`.
fn incomplete_envelope(answer: &str) -> String {
    trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
        ),
        server(1, answer),
    ])
}

/// A `-32021` whose `data` is `data`.
fn capability_error(data: &str) -> String {
    trace(&[
        client(
            0,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":-32021,"message":"x"{data}}}}}"#),
        ),
    ])
}

/// A `tools/call` declaring `capabilities`, answered by an `input_required`
/// result asking for `method`.
fn asks_for(capabilities: &str, method: &str) -> String {
    trace(&[
        client(
            0,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"t","_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{capabilities}}}}}}}"#
            ),
        ),
        server(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"input_required","inputRequests":{{"a":{{"method":"{method}"}}}}}}}}"#
            ),
        ),
    ])
}

/// A `subscriptions/listen` session carrying `notification` on its stream.
fn listening(notification: &str) -> String {
    trace(&[
        client(
            0,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"subscriptions/listen","params":{{{META}}}}}"#
            ),
        ),
        server(1, notification),
    ])
}

#[test]
fn the_traceparent_grammar_is_pinned_field_by_field() {
    assert!(validate_traceparent(&json!(VALID)).is_ok());

    let rejected = [
        // Wrong field count, either way.
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
        // Version: two lowercase hex digits.
        "0-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "0g-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "0A-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        // Trace id: 32 lowercase hex digits, never all zero.
        "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
        "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e473z-00f067aa0ba902b7-01",
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
        // Parent id: 16 lowercase hex digits, never all zero.
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00F067AA0BA902B7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
        // Flags: two lowercase hex digits.
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-1",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-0g",
    ];
    for value in rejected {
        assert!(
            validate_traceparent(&json!(value)).is_err(),
            "{value:?} should be rejected"
        );
    }

    // A non-string is reported as such rather than parsed.
    assert!(validate_traceparent(&json!(42)).is_err());
    assert!(validate_traceparent(&json!(null)).is_err());
}

#[test]
fn trace_context_is_read_from_both_envelopes_and_only_when_present() {
    let check = "meta.trace-context-format";

    // A malformed `traceparent` in request params.
    let request_side = trace(&[
        post(0, r#"{"mcp-method":"ping"}"#),
        client(
            1,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"traceparent":"nope"}}}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &request_side).len(), 1);

    // And in a result envelope, which is the other half of the clause.
    let result_side = trace(&[
        post(0, r#"{"mcp-method":"ping"}"#),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            2,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","_meta":{"traceparent":"nope"}}}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &result_side).len(), 1);

    // A valid one, and an envelope carrying none at all, are both silent.
    let valid = request_side.replace("nope", VALID);
    assert!(findings_for(check, &valid).is_empty());
    let absent = trace(&[
        post(0, r#"{"mcp-method":"ping"}"#),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
    ]);
    assert!(findings_for(check, &absent).is_empty());

    // `tracestate` and `baggage` are vendor-defined lists: only their gross
    // shape is judged, so a string passes and a non-string does not.
    let bad_state = request_side.replace(r#""traceparent":"nope""#, r#""tracestate":42"#);
    assert_eq!(findings_for(check, &bad_state).len(), 1);
    let good_state = request_side.replace(r#""traceparent":"nope""#, r#""baggage":"k=v""#);
    assert!(findings_for(check, &good_state).is_empty());
}

#[test]
fn a_malformed_envelope_must_draw_invalid_params() {
    let check = "meta.missing-required-field-rejected";

    // A result, or the wrong error code: both are failures to reject.
    assert_eq!(
        findings_for(
            check,
            &incomplete_envelope(r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#)
        )
        .len(),
        1
    );
    let findings = findings_for(
        check,
        &incomplete_envelope(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"x"}}"#),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("-32603"), "{findings:?}");

    // `-32602` is the required answer.
    assert!(
        findings_for(
            check,
            &incomplete_envelope(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"x"}}"#
            )
        )
        .is_empty()
    );

    // A complete envelope answered with a result is the normal case.
    let complete = trace(&[
        client(
            0,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert!(findings_for(check, &complete).is_empty());
}

#[test]
fn the_missing_capability_error_must_list_what_was_missing() {
    let check = "meta.missing-capability-error";

    // Absent, empty, and wrongly-typed lists are each their own finding.
    assert_eq!(findings_for(check, &capability_error("")).len(), 1);
    assert_eq!(
        findings_for(
            check,
            &capability_error(r#","data":{"requiredCapabilities":[]}"#)
        )
        .len(),
        1
    );
    assert_eq!(
        findings_for(
            check,
            &capability_error(r#","data":{"requiredCapabilities":"elicitation"}"#)
        )
        .len(),
        1
    );

    // A populated array is what the clause asks for.
    assert!(
        findings_for(
            check,
            &capability_error(r#","data":{"requiredCapabilities":["elicitation"]}"#)
        )
        .is_empty()
    );

    // Any other error code is outside this clause.
    let other = capability_error("").replace("-32021", "-32602");
    assert!(findings_for(check, &other).is_empty());
}

#[test]
fn an_input_request_is_matched_to_the_capability_it_needs() {
    let check = "meta.no-undeclared-capability-reliance";

    // Each of the three methods maps to its own capability name; asking for one
    // the request never declared is the violation.
    for (method, capability) in [
        ("elicitation/create", "elicitation"),
        ("sampling/createMessage", "sampling"),
        ("roots/list", "roots"),
    ] {
        assert_eq!(
            findings_for(check, &asks_for("{}", method)).len(),
            1,
            "{method} needs {capability}"
        );
        let declared = format!(r#"{{"{capability}":{{}}}}"#);
        assert!(
            findings_for(check, &asks_for(&declared, method)).is_empty(),
            "{method} is fine once {capability} is declared"
        );
        // Declaring a *different* capability does not cover it.
        assert_eq!(
            findings_for(check, &asks_for(r#"{"logging":{}}"#, method)).len(),
            1,
            "{method} is not covered by an unrelated capability"
        );
    }

    // A method outside the map needs no capability and is not reported.
    assert!(findings_for(check, &asks_for("{}", "tools/list")).is_empty());
}

#[test]
fn subscription_tagging_binds_only_listen_streams_and_only_untagged_notifications() {
    let check = "meta.subscription-id-present";

    // Untagged change notification: the violation.
    assert_eq!(
        findings_for(
            check,
            &listening(r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#)
        )
        .len(),
        1
    );

    // Tagged: silent.
    assert!(
        findings_for(
            check,
            &listening(
                r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed","params":{"_meta":{"io.modelcontextprotocol/subscriptionId":"s1"}}}"#
            )
        )
        .is_empty()
    );

    // Request-scoped notifications ride their request's stream, not the listen
    // stream, so neither of these is tagged and neither is a finding.
    for method in ["notifications/progress", "notifications/message"] {
        assert!(
            findings_for(
                check,
                &listening(&format!(r#"{{"jsonrpc":"2.0","method":"{method}"}}"#))
            )
            .is_empty(),
            "{method} is request-scoped"
        );
    }

    // A server *request* on the stream carries both an id and a method; it is
    // TRAN-066's defect, not an untagged notification.
    assert!(
        findings_for(
            check,
            &listening(r#"{"jsonrpc":"2.0","id":7,"method":"elicitation/create","params":{}}"#)
        )
        .is_empty()
    );

    // A message with neither an id nor a method is malformed, and BASE-047's.
    assert!(findings_for(check, &listening(r#"{"jsonrpc":"2.0"}"#)).is_empty());

    // A response is not a notification.
    assert!(
        findings_for(
            check,
            &listening(r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#)
        )
        .is_empty()
    );

    // Without a listen stream in the trace the clause does not bind at all.
    let unsubscribed = trace(&[
        client(
            0,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
        ),
    ]);
    assert!(findings_for(check, &unsubscribed).is_empty());
}

#[test]
fn the_http_status_clauses_judge_only_their_own_error_code() {
    // Both share one helper, so the code each passes is the only thing that
    // distinguishes them.
    for (check, code) in [
        ("meta.missing-required-field-http-status", -32602),
        ("meta.missing-capability-http-status", -32021),
    ] {
        let document = trace(&[
            client(
                0,
                &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
            ),
            status(1, 500),
            server(
                2,
                &format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":{code},"message":"x"}}}}"#),
            ),
        ]);
        assert_eq!(findings_for(check, &document).len(), 1, "{check}");
        assert!(
            findings_for(check, &document.replace("500", "400")).is_empty(),
            "{check} accepts 400"
        );
        // The sibling clause's code is not this one's business.
        let other = document.replace(&code.to_string(), "-32700");
        assert!(findings_for(check, &other).is_empty(), "{check}");
    }
}
