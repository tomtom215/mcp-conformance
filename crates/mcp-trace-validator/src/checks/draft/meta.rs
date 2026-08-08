// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `2026-07-28` `_meta`-envelope checks.
//!
//! The stateless rework moves per-session context onto every request, so these
//! read `params._meta` and `result._meta` and correlate a request with the
//! answer it drew. Each reports only what a recorded session shows: none of
//! them claims to prove a positive ("the server never relied on X"), which is
//! why the clauses that *only* have a positive form are excluded in the
//! registry rather than checked here.

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;
use mcp_conformance_core::trace::Direction;

/// Fields `2026-07-28` requires in every client request's `_meta`.
const REQUIRED_REQUEST_FIELDS: &[&str] = &[
    "io.modelcontextprotocol/protocolVersion",
    "io.modelcontextprotocol/clientCapabilities",
];

/// `MissingRequiredClientCapabilityError`.
const MISSING_CAPABILITY_CODE: i64 = -32021;
/// JSON-RPC `Invalid params`, which a malformed `_meta` envelope draws.
const INVALID_PARAMS: i64 = -32602;

/// The `_meta` object of a message's `params`, when present.
fn params_meta(payload: &Value) -> Option<&serde_json::Map<String, Value>> {
    payload.get("params")?.get("_meta")?.as_object()
}

/// Client requests in the trace, as `(seq, id, payload)`.
fn client_requests<'a>(
    context: &'a TraceContext<'_>,
) -> impl Iterator<Item = (u64, Option<&'a Value>, &'a Value)> + 'a {
    context.messages().filter_map(|(event, _, _)| {
        if !matches!(event.direction, Direction::ClientToServer) {
            return None;
        }
        let payload = event.message_payload()?;
        payload.get("method")?;
        Some((event.seq, payload.get("id"), payload))
    })
}

/// `BASE-030`: every client request carries the required `io.modelcontextprotocol/*`
/// fields in `_meta`.
///
/// Notifications are excluded: the clause binds *requests*, which the stateless
/// model defines as the messages a server must be able to process standalone.
pub(in crate::checks) fn required_request_fields(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, id, payload) in client_requests(context) {
        if id.is_none() {
            continue; // a notification, not a request
        }
        let meta = params_meta(payload);
        for field in REQUIRED_REQUEST_FIELDS {
            let present = meta.is_some_and(|meta| meta.contains_key(*field));
            if !present {
                sink.push(
                    Some(seq),
                    format!("request `_meta` is missing required field `{field}`"),
                );
            }
        }
    }
}

/// `BASE-031`: a request missing a required `_meta` field must draw `-32602`.
///
/// Falsified when the server answered such a request with a *result*, or with
/// some other error code — both of which the trace shows directly. A request
/// left unanswered inside the recording is not reported: the session may simply
/// have ended before the answer.
pub(in crate::checks) fn missing_required_field_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let malformed: BTreeMap<String, u64> = client_requests(context)
        .filter_map(|(seq, id, payload)| {
            let id = id?;
            let meta = params_meta(payload);
            let complete = REQUIRED_REQUEST_FIELDS
                .iter()
                .all(|field| meta.is_some_and(|meta| meta.contains_key(*field)));
            (!complete).then(|| (id.to_string(), seq))
        })
        .collect();
    if malformed.is_empty() {
        return;
    }
    for (event, _, _) in context.messages() {
        if !matches!(event.direction, Direction::ServerToClient) {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let Some(id) = payload.get("id") else {
            continue;
        };
        let Some(&request_seq) = malformed.get(&id.to_string()) else {
            continue;
        };
        match payload.get("error").and_then(|error| error.get("code")) {
            Some(code) if code.as_i64() == Some(INVALID_PARAMS) => {}
            Some(code) => sink.push(
                Some(event.seq),
                format!(
                    "request at seq {request_seq} was missing a required `_meta` field; \
                     the server answered with error code {code} rather than {INVALID_PARAMS}"
                ),
            ),
            None => sink.push(
                Some(event.seq),
                format!(
                    "request at seq {request_seq} was missing a required `_meta` field; \
                     the server answered with a result rather than error {INVALID_PARAMS}"
                ),
            ),
        }
    }
}

/// The HTTP status recorded closest after `seq`, when the trace carries one.
fn http_status_after(context: &TraceContext<'_>, seq: u64) -> Option<(u64, u16)> {
    context
        .events()
        .iter()
        .filter(|event| event.seq > seq)
        .find_map(|event| match &event.body {
            mcp_conformance_core::trace::EventBody::Http {
                status: Some(status),
                ..
            } => Some((event.seq, *status)),
            _ => None,
        })
}

/// Reports every server error carrying `code` whose HTTP response status is not
/// `400` — the shared body of `BASE-032` and `BASE-036`.
fn http_status_for_error(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    code: i64,
    clause: &str,
) {
    for (event, _, _) in context.messages() {
        if !matches!(event.direction, Direction::ServerToClient) {
            continue;
        }
        let matches_code = event
            .message_payload()
            .and_then(|payload| payload.get("error"))
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64)
            == Some(code);
        if !matches_code {
            continue;
        }
        // Only judged when the recording actually carries HTTP framing; on stdio
        // there is no status to check, and a trace without one evidences nothing.
        if let Some((status_seq, status)) = http_status_after(context, event.seq)
            && status != 400
        {
            sink.push(
                Some(status_seq),
                format!("{clause}: error {code} was returned with HTTP {status}, not 400"),
            );
        }
    }
}

/// `BASE-032`: on HTTP, a `-32602` for a malformed `_meta` envelope is a `400`.
pub(in crate::checks) fn missing_required_field_http_status(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    http_status_for_error(
        context,
        sink,
        INVALID_PARAMS,
        "missing required `_meta` field",
    );
}

/// `BASE-036`: on HTTP, `MissingRequiredClientCapabilityError` is a `400`.
pub(in crate::checks) fn missing_capability_http_status(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    http_status_for_error(
        context,
        sink,
        MISSING_CAPABILITY_CODE,
        "missing required client capability",
    );
}

/// `BASE-035`: a `-32021` must carry `data.requiredCapabilities` listing what
/// was missing.
///
/// The trace cannot show that the server *needed* a capability, so the positive
/// direction is out of reach; what it can show is a `-32021` whose shape does
/// not carry the list the clause requires.
pub(in crate::checks) fn missing_capability_error(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, _, _) in context.messages() {
        let Some(error) = event
            .message_payload()
            .and_then(|payload| payload.get("error"))
        else {
            continue;
        };
        if error.get("code").and_then(Value::as_i64) != Some(MISSING_CAPABILITY_CODE) {
            continue;
        }
        match error
            .get("data")
            .and_then(|data| data.get("requiredCapabilities"))
        {
            Some(list) if list.is_array() => {
                if list.as_array().is_some_and(Vec::is_empty) {
                    sink.push(
                        Some(event.seq),
                        format!(
                            "error {MISSING_CAPABILITY_CODE} carries an empty \
                             `data.requiredCapabilities`; it must list the missing capabilities"
                        ),
                    );
                }
            }
            Some(_) => sink.push(
                Some(event.seq),
                format!(
                    "error {MISSING_CAPABILITY_CODE} has `data.requiredCapabilities` \
                     that is not an array"
                ),
            ),
            None => sink.push(
                Some(event.seq),
                format!(
                    "error {MISSING_CAPABILITY_CODE} has no `data.requiredCapabilities` \
                     listing the missing capabilities"
                ),
            ),
        }
    }
}

/// `BASE-034`: a server must not rely on capabilities the client did not declare.
///
/// Reliance is internal, so this reports its one wire-visible form: the server
/// asking the client for input (`resultType: "input_required"`, SEP-2322) of a
/// kind the request's own `clientCapabilities` never advertised.
pub(in crate::checks) fn no_undeclared_capability_reliance(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Capabilities declared per request id, from the request's own `_meta` —
    // there is no session-wide declaration to fall back on at this revision.
    let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_, id, payload) in client_requests(context) {
        let Some(id) = id else { continue };
        let names = params_meta(payload)
            .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
            .and_then(Value::as_object)
            .map(|caps| caps.keys().cloned().collect())
            .unwrap_or_default();
        declared.insert(id.to_string(), names);
    }
    for (event, _, _) in context.messages() {
        if !matches!(event.direction, Direction::ServerToClient) {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let Some(result) = payload.get("result") else {
            continue;
        };
        if result.get("resultType").and_then(Value::as_str) != Some("input_required") {
            continue;
        }
        let Some(id) = payload.get("id") else {
            continue;
        };
        let Some(declared) = declared.get(&id.to_string()) else {
            continue;
        };
        let requests = result
            .get("inputRequests")
            .and_then(Value::as_object)
            .map(|map| map.values().collect::<Vec<_>>())
            .unwrap_or_default();
        for request in requests {
            let Some(method) = request.get("method").and_then(Value::as_str) else {
                continue;
            };
            let needed = match method {
                "elicitation/create" => "elicitation",
                "sampling/createMessage" => "sampling",
                "roots/list" => "roots",
                _ => continue,
            };
            if !declared.iter().any(|name| name == needed) {
                sink.push(
                    Some(event.seq),
                    format!(
                        "server asked for `{method}`, which needs the `{needed}` capability, \
                         but the request's `clientCapabilities` did not declare it"
                    ),
                );
            }
        }
    }
}

/// `BASE-039`: notifications on a `subscriptions/listen` stream carry
/// `io.modelcontextprotocol/subscriptionId`.
///
/// Scoped to traces that actually opened such a stream: without one, a
/// notification is request-scoped (progress, logging) and the clause does not
/// bind it.
pub(in crate::checks) fn subscription_id_present(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let listening = context.messages().any(|(_, _, _)| false)
        || context.messages().any(|(event, _, _)| {
            event
                .message_payload()
                .and_then(|payload| payload.get("method"))
                .and_then(Value::as_str)
                == Some("subscriptions/listen")
        });
    if !listening {
        return;
    }
    for (event, _, _) in context.messages() {
        if !matches!(event.direction, Direction::ServerToClient) {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        // A notification: a method with no id.
        if payload.get("id").is_some() || payload.get("method").is_none() {
            continue;
        }
        let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
        // Request-scoped notifications ride their request's response stream and
        // are outside this clause.
        if method.starts_with("notifications/progress")
            || method.starts_with("notifications/message")
        {
            continue;
        }
        let tagged = params_meta(payload)
            .is_some_and(|meta| meta.contains_key("io.modelcontextprotocol/subscriptionId"));
        if !tagged {
            sink.push(
                Some(event.seq),
                format!(
                    "notification `{method}` on a subscriptions/listen stream has no \
                     `io.modelcontextprotocol/subscriptionId` in `_meta`"
                ),
            );
        }
    }
}

/// `BASE-040`: `traceparent` and `tracestate`/`baggage` follow their W3C formats.
///
/// Only the `traceparent` grammar is fixed enough to judge from a trace: version
/// `00`, a 32-hex trace id that is not all zeroes, a 16-hex parent id that is not
/// all zeroes, and 2 hex flags. `tracestate` and `baggage` are list formats whose
/// members are vendor-defined, so only their gross shape is checked.
pub(in crate::checks) fn trace_context_format(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        for envelope in ["params", "result"] {
            let meta = payload
                .get(envelope)
                .and_then(|member| member.get("_meta"))
                .and_then(Value::as_object);
            let Some(meta) = meta else { continue };
            if let Some(value) = meta.get("traceparent")
                && let Err(reason) = validate_traceparent(value)
            {
                sink.push(
                    Some(event.seq),
                    format!("{envelope}._meta.traceparent {reason}"),
                );
            }
            for key in ["tracestate", "baggage"] {
                if let Some(value) = meta.get(key)
                    && !value.is_string()
                {
                    sink.push(
                        Some(event.seq),
                        format!("{envelope}._meta.{key} is not a string"),
                    );
                }
            }
        }
    }
}

/// The W3C Trace Context `traceparent` grammar, version `00`.
fn validate_traceparent(value: &Value) -> Result<(), String> {
    let Some(text) = value.as_str() else {
        return Err("is not a string".to_owned());
    };
    let parts: Vec<&str> = text.split('-').collect();
    let [version, trace_id, parent_id, flags] = parts.as_slice() else {
        return Err(format!(
            "is {text:?}; W3C Trace Context requires four `-`-separated fields"
        ));
    };
    let hex = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    };
    if version.len() != 2 || !hex(version) {
        return Err(format!(
            "has version {version:?}; expected two lowercase hex digits"
        ));
    }
    if trace_id.len() != 32 || !hex(trace_id) {
        return Err(format!(
            "has trace-id {trace_id:?}; expected 32 lowercase hex digits"
        ));
    }
    if trace_id.bytes().all(|b| b == b'0') {
        return Err("has an all-zero trace-id, which W3C Trace Context forbids".to_owned());
    }
    if parent_id.len() != 16 || !hex(parent_id) {
        return Err(format!(
            "has parent-id {parent_id:?}; expected 16 lowercase hex digits"
        ));
    }
    if parent_id.bytes().all(|b| b == b'0') {
        return Err("has an all-zero parent-id, which W3C Trace Context forbids".to_owned());
    }
    if flags.len() != 2 || !hex(flags) {
        return Err(format!(
            "has flags {flags:?}; expected two lowercase hex digits"
        ));
    }
    Ok(())
}
