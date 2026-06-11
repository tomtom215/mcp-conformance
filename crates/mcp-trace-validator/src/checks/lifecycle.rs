// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` session lifecycle requirements (`LIFE-*`).
//!
//! These lean on [`TraceContext`]'s precomputed phases: every check sees the lifecycle
//! phase *before* each event, which is exactly the state the spec's ordering rules are
//! written against.

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::FindingSink;
use crate::context::{Phase, TraceContext};

/// `LIFE-001`: "The initialization phase MUST be the first interaction between client
/// and server." — the first message in the trace must be the client's `initialize`
/// request. An empty trace passes vacuously.
pub(super) fn first_interaction_initialize(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((event, kind, _)) = context.messages().next() else {
        return;
    };
    match (event.direction, kind) {
        (Direction::ClientToServer, MessageKind::Request { method, .. })
            if *method == "initialize" => {}
        (Direction::ClientToServer, MessageKind::Request { method, .. }) => sink.push(
            Some(event.seq),
            format!("first message is a {method:?} request, expected \"initialize\""),
        ),
        (direction, _) => sink.push(
            Some(event.seq),
            format!(
                "first message is {} ({}), expected the client's \"initialize\" request",
                describe_kind(kind),
                direction_name(direction)
            ),
        ),
    }
}

/// `LIFE-002`: the `initialize` request must carry `protocolVersion`, `capabilities`,
/// and `clientInfo` params.
pub(super) fn initialize_params(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((seq, params)) = context.initialize().request else {
        return; // No initialize at all: LIFE-001's finding.
    };
    let Some(params) = params else {
        sink.push(
            Some(seq),
            "initialize request has no params; protocolVersion, capabilities, and clientInfo are required".to_owned(),
        );
        return;
    };
    expect_member(
        sink,
        seq,
        params,
        "protocolVersion",
        Value::is_string,
        "a string",
    );
    expect_member(
        sink,
        seq,
        params,
        "capabilities",
        Value::is_object,
        "an object",
    );
    expect_member(
        sink,
        seq,
        params,
        "clientInfo",
        Value::is_object,
        "an object",
    );
}

fn expect_member(
    sink: &mut FindingSink,
    seq: u64,
    params: &Value,
    member: &str,
    predicate: fn(&Value) -> bool,
    expected: &str,
) {
    match params.get(member) {
        None => sink.push(
            Some(seq),
            format!("initialize params lack the {member} member"),
        ),
        Some(value) if !predicate(value) => sink.push(
            Some(seq),
            format!("initialize params member {member} should be {expected}"),
        ),
        Some(_) => {}
    }
}

/// `LIFE-003`: "After successful initialization, the client MUST send an `initialized`
/// notification …".
pub(super) fn initialized_notification(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let init = context.initialize();
    if let Some((result_seq, _)) = init.result
        && init.initialized.is_none()
    {
        sink.push(
                Some(result_seq),
                "the server answered initialize here, but no notifications/initialized notification follows in the trace".to_owned(),
            );
    }
}

/// `LIFE-004`: "The client SHOULD NOT send requests other than pings before the server
/// has responded to the `initialize` request."
pub(super) fn client_requests_before_init_response(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, kind, phase) in context.messages() {
        if event.direction != Direction::ClientToServer {
            continue;
        }
        if !matches!(
            phase,
            Phase::BeforeInitialize | Phase::AwaitingInitializeResult
        ) {
            continue;
        }
        if let MessageKind::Request { method, .. } = kind
            && *method != "initialize"
            && *method != "ping"
        {
            sink.push(
                Some(event.seq),
                format!(
                    "client sent a {method:?} request before the server responded to initialize"
                ),
            );
        }
    }
}

/// `LIFE-005`: "The server SHOULD NOT send requests other than pings and logging before
/// receiving the `initialized` notification."
///
/// In `2025-11-25`, logging travels as `notifications/message` — a notification, which
/// this requests-only check never flags — so the spec's "and logging" allowance needs
/// no special case here.
pub(super) fn server_requests_before_initialized(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, kind, phase) in context.messages() {
        if event.direction != Direction::ServerToClient || phase == Phase::Ready {
            continue;
        }
        if let MessageKind::Request { method, .. } = kind
            && *method != "ping"
        {
            sink.push(
                Some(event.seq),
                format!(
                    "server sent a {method:?} request before receiving the initialized notification"
                ),
            );
        }
    }
}

/// `LIFE-007`: "In the `initialize` request, the client MUST send a protocol version
/// it supports." — presence and string-ness of the version is the wire-observable
/// core; whether the client truly *supports* the version it sent is not in the trace.
pub(super) fn initialize_protocol_version(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((seq, params)) = context.initialize().request else {
        return; // No initialize at all: LIFE-001's finding.
    };
    match params.and_then(|params| params.get("protocolVersion")) {
        None => sink.push(
            Some(seq),
            "initialize request sends no protocolVersion".to_owned(),
        ),
        Some(Value::String(_)) => {}
        Some(other) => sink.push(
            Some(seq),
            format!("initialize request protocolVersion is {other}, expected a version string"),
        ),
    }
}

/// `LIFE-006`: the server's `initialize` result must carry a `protocolVersion` that is
/// a dated revision identifier. Whether the *negotiation* (same-version-if-supported)
/// was honored is not judgeable from a single trace; the shape and format are.
pub(super) fn initialize_result_version(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((seq, result)) = context.initialize().result else {
        return;
    };
    match result.get("protocolVersion") {
        None => sink.push(
            Some(seq),
            "initialize result lacks the protocolVersion member".to_owned(),
        ),
        Some(Value::String(version)) => {
            if version.parse::<ProtocolRevision>().is_err() {
                sink.push(
                    Some(seq),
                    format!(
                        "initialize result protocolVersion {version:?} is not a dated revision identifier (YYYY-MM-DD)"
                    ),
                );
            }
        }
        Some(other) => sink.push(
            Some(seq),
            format!("initialize result protocolVersion is {other}, expected a revision string"),
        ),
    }
}

/// `LIFE-010`: the initialize result must carry the server's capabilities and
/// implementation information (`capabilities` and `serverInfo` objects).
///
/// A missing or error-answered initialize exchange is owned by the handshake
/// checks (LIFE-001/003/006); this one judges only a result that exists.
pub(super) fn initialize_result_shape(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((seq, result)) = context.initialize().result else {
        return;
    };
    for (member, label) in [
        ("capabilities", "its capabilities"),
        ("serverInfo", "its implementation information (serverInfo)"),
    ] {
        match result.get(member) {
            None => sink.push(
                Some(seq),
                format!("initialize result lacks {label}: no {member} member"),
            ),
            Some(value) if !value.is_object() => sink.push(
                Some(seq),
                format!("initialize result {member} is {value}, expected an object"),
            ),
            Some(_) => {}
        }
    }
}

const fn describe_kind(kind: &MessageKind<'_>) -> &'static str {
    match kind {
        MessageKind::Request { .. } => "a request",
        MessageKind::Notification { .. } => "a notification",
        MessageKind::Result { .. } => "a result response",
        MessageKind::Error { .. } => "an error response",
        MessageKind::Invalid { .. } => "not a valid JSON-RPC message",
        // MessageKind is #[non_exhaustive]; future shapes still deserve a description.
        _ => "an unrecognized message kind",
    }
}

const fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::ClientToServer => "client to server",
        Direction::ServerToClient => "server to client",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn describe_kind_names_every_shape_exactly() {
        // These strings appear verbatim in findings; mutating any arm must fail here.
        let request = json!({"id": 1, "method": "x"});
        let notification = json!({"method": "x"});
        let result = json!({"id": 1, "result": {}});
        let error = json!({"id": 1, "error": {}});
        let invalid = json!([]);
        let cases = [
            (&request, "a request"),
            (&notification, "a notification"),
            (&result, "a result response"),
            (&error, "an error response"),
            (&invalid, "not a valid JSON-RPC message"),
        ];
        for (payload, expected) in cases {
            let kind = mcp_conformance_core::message::classify(payload);
            assert_eq!(describe_kind(&kind), expected, "for {payload}");
        }
    }

    #[test]
    fn direction_name_is_exact() {
        assert_eq!(
            direction_name(Direction::ClientToServer),
            "client to server"
        );
        assert_eq!(
            direction_name(Direction::ServerToClient),
            "server to client"
        );
    }

    #[test]
    fn initialize_params_with_wrong_types_are_flagged() {
        // Present-but-mistyped members must be findings, not silent passes — this
        // pins the type-predicate guard in expect_member.
        use crate::context::TraceContext;
        use crate::reader::{Limits, parse_trace};
        let doc = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":123,"capabilities":[],"clientInfo":"nope"}}}"#;
        let events = parse_trace(doc, &Limits::default()).expect("valid trace");
        let context = TraceContext::new(&events);
        let findings = crate::checks::find("lifecycle.initialize-params")
            .expect("check exists")
            .run(&context);
        assert_eq!(findings.len(), 3, "{findings:?}");
        assert!(
            findings[0]
                .detail
                .contains("protocolVersion should be a string")
        );
        assert!(
            findings[1]
                .detail
                .contains("capabilities should be an object")
        );
        assert!(
            findings[2]
                .detail
                .contains("clientInfo should be an object")
        );
    }

    #[test]
    fn initialize_result_shape_demands_capability_and_serverinfo_objects() {
        fn handshake_with_result(result: &str) -> String {
            let request = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#;
            format!(
                "{request}\n{{\"seq\":1,\"direction\":\"server-to-client\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{result}}}}}"
            )
        }
        let run = |result: &str| {
            let trace = handshake_with_result(result);
            let events = crate::reader::parse_trace(&trace, &crate::reader::Limits::default())
                .expect("trace parses");
            let context = TraceContext::new(&events);
            crate::checks::find("lifecycle.initialize-result-shape")
                .expect("check registered")
                .run(&context)
        };

        // Complete shape: no findings.
        assert!(
            run(r#"{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}"#)
                .is_empty()
        );
        // Missing capabilities only.
        let missing_caps =
            run(r#"{"protocolVersion":"2025-11-25","serverInfo":{"name":"s","version":"0"}}"#);
        assert_eq!(missing_caps.len(), 1, "{missing_caps:?}");
        assert!(missing_caps[0].detail.contains("capabilities"));
        // Missing serverInfo only.
        let missing_info = run(r#"{"protocolVersion":"2025-11-25","capabilities":{}}"#);
        assert_eq!(missing_info.len(), 1, "{missing_info:?}");
        assert!(missing_info[0].detail.contains("serverInfo"));
        // Wrong types are findings too, one per member.
        let wrong = run(r#"{"capabilities":7,"serverInfo":"s"}"#);
        assert_eq!(wrong.len(), 2, "{wrong:?}");
        // No initialize result at all: the handshake checks own that case.
        let events = crate::reader::parse_trace("", &crate::reader::Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        assert!(
            crate::checks::find("lifecycle.initialize-result-shape")
                .unwrap()
                .run(&context)
                .is_empty()
        );
    }
}
