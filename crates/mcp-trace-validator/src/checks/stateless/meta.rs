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
use super::http_status_for;
use crate::context::TraceContext;
use mcp_conformance_core::trace::Direction;

mod trace_context;

#[cfg(test)]
mod tests;

pub(in crate::checks) use trace_context::trace_context_format;

/// Fields `2026-07-28` requires in every client request's `_meta`.
const REQUIRED_REQUEST_FIELDS: &[&str] = &[
    "io.modelcontextprotocol/protocolVersion",
    "io.modelcontextprotocol/clientCapabilities",
];

/// `MissingRequiredClientCapabilityError`.
const MISSING_CAPABILITY_CODE: i64 = -32021;
/// JSON-RPC `Invalid params`, which a malformed `_meta` envelope draws.
const INVALID_PARAMS: i64 = -32602;

/// The handshake this revision removed. A request for it is the previous era's
/// opener, not a `2026-07-28` request with a malformed envelope.
const LEGACY_HANDSHAKE: &str = "initialize";

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
        sink.examined();
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

/// Client requests whose `_meta` is missing a required field, by id text.
///
/// Shared by `BASE-031` (what such a request must draw) and `BASE-032` (what
/// HTTP status that answer must ride), because both clauses are about the
/// *same* request and neither binds a `-32602` raised for any other reason.
fn malformed_requests(context: &TraceContext<'_>) -> BTreeMap<String, u64> {
    client_requests(context)
        .filter_map(|(seq, id, payload)| {
            let id = id?;
            // The one exchange the specification takes out of this rule: a legacy
            // `initialize` arriving at a modern server. `basic/versioning`'s
            // compatibility matrix states that there "the exact code is
            // implementation-defined (`initialize` is an unknown method and the
            // request also lacks the required `_meta` fields)" — two rules apply
            // and the specification declines to pick, so a server answering
            // `-32601` conforms. Without this, every cross-era capture would
            // carry a MUST failure the specification has explicitly waived. The
            // client's own defect is still reported, by BASE-030.
            if payload.get("method").and_then(Value::as_str) == Some(LEGACY_HANDSHAKE) {
                return None;
            }
            let meta = params_meta(payload);
            let complete = REQUIRED_REQUEST_FIELDS
                .iter()
                .all(|field| meta.is_some_and(|meta| meta.contains_key(*field)));
            (!complete).then(|| (id.to_string(), seq))
        })
        .collect()
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
    let malformed = malformed_requests(context);
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
        // The subject is an *answer* to a malformed request; one the recording
        // never saw answered settles nothing.
        sink.examined();
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

/// Reports every server error carrying `code` whose HTTP response status is not
/// `400` — the shared body of `BASE-032` and `BASE-036`.
///
/// `answering` narrows which errors of that code the clause reaches, by the id
/// of the request each answers. `BASE-036` passes `None`: `-32021` has exactly
/// one cause, so every one of them is its subject. `BASE-032` passes the
/// malformed-request set, because its `-32602` is not the only `-32602` a
/// conforming server emits — this revision *replaced* `-32002` with it, so a
/// resource-not-found now carries the same code, and the clause says nothing
/// about that answer's HTTP status.
fn http_status_for_error(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    code: i64,
    clause: &str,
    answering: Option<&BTreeMap<String, u64>>,
) {
    for (event, _, _) in context.messages() {
        if !matches!(event.direction, Direction::ServerToClient) {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let matches_code = payload
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64)
            == Some(code);
        if !matches_code {
            continue;
        }
        if let Some(answering) = answering {
            let answers_a_subject = payload
                .get("id")
                .is_some_and(|id| answering.contains_key(&id.to_string()));
            if !answers_a_subject {
                continue;
            }
        }
        // Only judged when the recording actually carries HTTP framing; on stdio
        // there is no status to check, and a trace without one evidences nothing.
        let Some((status_seq, status)) = http_status_for(context, event.seq) else {
            continue;
        };
        sink.examined();
        if status != 400 {
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
    // Narrowed to the errors this clause is about. Before the enriched HTTP
    // capture carried one, every `-32602` in every recording *was* a malformed
    // envelope, so the difference could not show; a server answering a
    // resource-not-found `-32602` with anything but 400 would have been
    // reported for a clause that does not bind it.
    let malformed = malformed_requests(context);
    http_status_for_error(
        context,
        sink,
        INVALID_PARAMS,
        "missing required `_meta` field",
        Some(&malformed),
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
        None,
    );
}

/// `BASE-035`: a `-32021` must carry `data.requiredCapabilities` naming what
/// was missing.
///
/// The trace cannot show that the server *needed* a capability, so the positive
/// direction is out of reach; what it can show is a `-32021` whose shape does
/// not carry what the clause requires.
///
/// The clause's word is "lists", and this check read that as a JSON array until
/// 2026-08-17. The schema disagrees, and the schema is the authority:
/// `MissingRequiredClientCapabilityError.error.data.requiredCapabilities` is
/// typed [`ClientCapabilities`][schema] — the same nested object a client sends
/// in its `_meta`, carrying the *shape* of what is missing rather than a list of
/// names. Judged as an array, this check reported a conforming server, which is
/// the worst thing a conformance check can do; it now requires an object.
///
/// An empty object is still reported. `{}` declares nothing missing, so it
/// leaves the client with no more information than an error carrying no `data`
/// at all — the very thing the clause exists to prevent.
///
/// [schema]: https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2026-07-28/schema.ts
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
        sink.examined();
        match error
            .get("data")
            .and_then(|data| data.get("requiredCapabilities"))
        {
            Some(required) if required.is_object() => {
                if required.as_object().is_some_and(serde_json::Map::is_empty) {
                    sink.push(
                        Some(event.seq),
                        format!(
                            "error {MISSING_CAPABILITY_CODE} carries an empty \
                             `data.requiredCapabilities`; it must name the missing capabilities"
                        ),
                    );
                }
            }
            Some(_) => sink.push(
                Some(event.seq),
                format!(
                    "error {MISSING_CAPABILITY_CODE} has `data.requiredCapabilities` \
                     that is not a `ClientCapabilities` object"
                ),
            ),
            None => sink.push(
                Some(event.seq),
                format!(
                    "error {MISSING_CAPABILITY_CODE} has no `data.requiredCapabilities` \
                     naming the missing capabilities"
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
            // The subject is an input request of a kind a capability governs;
            // the revision's other input kinds need none.
            sink.examined();
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
    let listening = context.messages().any(|(event, _, _)| {
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
        sink.examined();
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
