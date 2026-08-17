// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `2026-07-28` Streamable HTTP checks.
//!
//! None of these may lean on an `initialize` exchange: the revision removes the
//! handshake, so a check gated on it is inert here and — worse than being
//! absent — reports a vacuous *pass*. That is why
//! `transport.protocol-version-header-present` exists rather than reusing the
//! `2025-11-25` header check, which returns early unless the trace negotiated.
//!
//! Split by subject. This file holds what the area shares: the POST pairing,
//! header safety, and the *mirror* vocabulary that names each body value a POST
//! must carry in a header. [`headers`] holds the request-header clauses the
//! client owns, [`stream`] the response-stream clauses, and [`validation`] the
//! rejection clauses the server owns.

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::support::decode_base64;
use crate::context::TraceContext;
use mcp_conformance_core::trace::{Direction, EventBody, TransportKind};

mod headers;
mod stdio;
mod stream;
mod validation;

#[cfg(test)]
mod tests;

pub(in crate::checks) use headers::{
    header_value_encoding, protocol_version_header_matches_body, protocol_version_header_present,
    request_metadata_headers, sentinel_marker_case, sentinel_pattern_encoded,
    x_mcp_header_mirrored, x_mcp_header_name_valid,
};
pub(in crate::checks) use stdio::{
    cancel_notification_references_request, no_messages_after_cancel_notification,
};
pub(in crate::checks) use stream::{
    accel_buffering_header, client_no_responses, no_independent_server_requests,
    no_messages_after_cancellation,
};
pub(in crate::checks) use validation::{
    header_body_match_validated, header_mismatch_status, invalid_param_header_rejected,
    unknown_method_404, unsupported_version_error, unsupported_version_status,
    version_mismatch_rejected,
};

/// `Mcp-Name`'s source field, by method — the Standard Request Headers table.
pub(super) const NAME_SOURCED: &[(&str, &str)] = &[
    ("tools/call", "name"),
    ("prompts/get", "name"),
    ("resources/read", "uri"),
];

/// The `_meta` key carrying a request's protocol version.
pub(super) const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// The Base64 sentinel's opening marker.
const SENTINEL_OPEN: &str = "=?base64?";
/// The sentinel's closing marker.
const SENTINEL_CLOSE: &str = "?=";

/// A client POST: the HTTP headers it carried, and the message they framed.
#[derive(Debug, Clone, Copy)]
pub(super) struct Post<'a> {
    /// The `seq` of the HTTP event carrying the headers.
    pub seq: u64,
    /// The `seq` of the message event the POST framed.
    pub message_seq: u64,
    /// The POST's headers. The trace reader lowercases the keys, so lookups
    /// here are by lowercase name and on-the-wire casing cannot hide anything.
    pub headers: &'a BTreeMap<String, String>,
    /// The JSON-RPC message the POST carried.
    pub payload: &'a Value,
}

impl Post<'_> {
    /// The message's `method`, when it has one.
    pub(super) fn method(&self) -> Option<&str> {
        self.payload.get("method").and_then(Value::as_str)
    }

    /// Whether the POST carried a JSON-RPC *request* rather than a notification.
    ///
    /// The header clauses are scoped to requests deliberately: the revision
    /// states that "header requirements for notification POSTs are not defined
    /// by this revision" (`#sending-messages`), so judging one would invent a
    /// rule the specification declines to make.
    pub(super) fn is_request(&self) -> bool {
        self.method().is_some() && self.payload.get("id").is_some_and(|id| !id.is_null())
    }

    /// The protocol version this request's `_meta` envelope states.
    pub(super) fn body_protocol_version(&self) -> Option<&str> {
        self.payload
            .get("params")?
            .get("_meta")?
            .get(META_PROTOCOL_VERSION)?
            .as_str()
    }
}

/// Every client POST in the trace, in capture order.
///
/// The tap records an `http` event and then the message it carried, so the
/// pairing is "the next client message after this client `http` event". A trace
/// without HTTP framing (stdio) yields nothing, which is correct: these clauses
/// bind the Streamable HTTP transport only.
pub(super) fn posts<'a>(context: &'a TraceContext<'_>) -> Vec<Post<'a>> {
    let mut out = Vec::new();
    let events = context.events();
    for (index, event) in events.iter().enumerate() {
        if event.direction != Direction::ClientToServer
            || event.transport != TransportKind::StreamableHttp
        {
            continue;
        }
        let EventBody::Http { headers, .. } = &event.body else {
            continue;
        };
        let framed = events[index + 1..]
            .iter()
            .find(|later| later.direction == Direction::ClientToServer);
        if let Some(framed) = framed
            && let Some(payload) = framed.message_payload()
        {
            out.push(Post {
                seq: event.seq,
                message_seq: framed.seq,
                headers,
                payload,
            });
        }
    }
    out
}

/// The POSTs of the trace keyed by the `seq` of the message each framed — the
/// index the server-side checks need to walk back from an answer to its request.
pub(super) fn posts_by_message<'a>(context: &'a TraceContext<'_>) -> BTreeMap<u64, Post<'a>> {
    posts(context)
        .into_iter()
        .map(|post| (post.message_seq, post))
        .collect()
}

/// Whether `value` is safely representable as a plain ASCII header value.
///
/// RFC 9110 admits visible ASCII (`0x21`–`0x7E`), space and horizontal tab; the
/// revision adds that a value with leading or trailing whitespace cannot be
/// carried plainly either (`#value-encoding`).
pub(super) fn header_safe(value: &str) -> bool {
    value.trim() == value
        && value
            .bytes()
            .all(|byte| byte == 0x09 || (0x20..0x7f).contains(&byte))
}

/// The Base64 payload `value` carries, when it uses the sentinel exactly.
pub(super) fn sentinel_payload(value: &str) -> Option<&str> {
    value
        .strip_prefix(SENTINEL_OPEN)
        .and_then(|rest| rest.strip_suffix(SENTINEL_CLOSE))
}

/// Whether `value` uses the sentinel markers in any casing but the required one.
///
/// The markers "are case-sensitive and MUST appear exactly as shown (lowercase)"
/// (TRAN-089), so a value that only case-insensitively matches is a distinct,
/// nameable defect rather than an arbitrary plain value.
pub(super) fn is_miscased_sentinel(value: &str) -> bool {
    let folded = value.to_ascii_lowercase();
    sentinel_payload(value).is_none() && sentinel_payload(&folded).is_some()
}

/// How a mirrored header value relates to the body value it mirrors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Match {
    /// The header carries the body value, plainly or Base64-encoded.
    Carried,
    /// The header repeats the body value verbatim even though it matches the
    /// sentinel pattern, which TRAN-092 requires the client to encode.
    UnencodedSentinel,
    /// The header and the body disagree.
    Mismatch,
}

/// Compares a header value against the body value it mirrors, decoding the
/// sentinel first — the comparison the specification requires of servers
/// (TRAN-091, TRAN-103) and therefore the one a trace must be judged by.
pub(super) fn compare(header: &str, body: &str) -> Match {
    if header == body {
        return if sentinel_payload(body).is_some() {
            Match::UnencodedSentinel
        } else {
            Match::Carried
        };
    }
    if let Some(encoded) = sentinel_payload(header)
        && decode_base64(encoded).as_deref() == Some(body)
    {
        return Match::Carried;
    }
    Match::Mismatch
}

/// One `x-mcp-header` annotation found in a tool's `inputSchema`.
#[derive(Debug, Clone)]
pub(super) struct Designation {
    /// The chain of `properties` keys leading to the annotated property.
    pub path: Vec<String>,
    /// The `x-mcp-header` value, verbatim.
    pub name: String,
    /// The header it constructs, lowercased for lookup against a trace's headers.
    pub header: String,
    /// The annotated property's declared `type`, when it declares one.
    pub declared_type: Option<String>,
}

/// Every tool definition the trace carried, as `(seq of the result, tool)`.
pub(super) fn tool_definitions<'a>(
    context: &'a TraceContext<'_>,
) -> impl Iterator<Item = (u64, &'a Value)> + 'a {
    context.messages().flat_map(|(event, _, _)| {
        event
            .message_payload()
            .and_then(|payload| payload.get("result"))
            .and_then(|result| result.get("tools"))
            .and_then(Value::as_array)
            .map(|tools| tools.iter().map(move |tool| (event.seq, tool)))
            .into_iter()
            .flatten()
    })
}

/// The `x-mcp-header` annotations `schema` declares, in schema order.
///
/// Walks chains of `properties` keys only — the specification's *statically
/// reachable* definition (`#schema-extension`). An annotation anywhere else is
/// TRAN-082's business, which the registry excludes. Recursion is bounded by the
/// JSON parser's own nesting limit, applied by the reader before any check runs.
pub(super) fn designations(schema: &Value) -> Vec<Designation> {
    let mut out = Vec::new();
    collect_designations(schema, &mut Vec::new(), &mut out);
    out
}

/// The recursive half of [`designations`].
fn collect_designations(schema: &Value, path: &mut Vec<String>, out: &mut Vec<Designation>) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (property, subschema) in properties {
        path.push(property.clone());
        if let Some(name) = subschema.get("x-mcp-header").and_then(Value::as_str) {
            out.push(Designation {
                path: path.clone(),
                name: name.to_owned(),
                header: format!("mcp-param-{}", name.to_ascii_lowercase()),
                declared_type: subschema
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
        collect_designations(subschema, path, out);
        path.pop();
    }
}

/// The designations declared per tool name, across every tool list in the trace.
pub(in crate::checks::draft) fn designations_by_tool(
    context: &TraceContext<'_>,
) -> BTreeMap<String, Vec<Designation>> {
    let mut out = BTreeMap::new();
    for (_, tool) in tool_definitions(context) {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(schema) = tool.get("inputSchema") else {
            continue;
        };
        let declared = designations(schema);
        if !declared.is_empty() {
            out.insert(name.to_owned(), declared);
        }
    }
    out
}

/// The instance value at a designation's exact property path, when present.
fn value_at<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cursor = root;
    for step in path {
        cursor = cursor.get(step)?;
    }
    Some(cursor)
}

/// A parameter value's header form, per the Value Encoding type conversions:
/// strings as-is, integers as a decimal string, booleans lowercase.
///
/// Any other JSON type has no defined header form — the specification permits
/// annotating primitives only — so it yields `None` and comparison abstains.
fn header_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) if number.is_i64() || number.is_u64() => Some(number.to_string()),
        _ => None,
    }
}

/// One body value a POST is required to mirror into an HTTP header.
#[derive(Debug, Clone)]
pub(super) struct Mirror {
    /// The header's lowercase name, for lookup against a trace's headers.
    pub header: String,
    /// The header as the specification spells it, for findings.
    pub label: String,
    /// The body path the value is sourced from, for findings.
    pub source: String,
    /// The body value's header form.
    pub value: String,
    /// Whether the Base64 sentinel may carry this header's value.
    pub encodable: bool,
}

/// The mirrors `post` must satisfy, given the designations the trace declared.
///
/// Each is *sourced*: a mirror exists only where the body actually carries the
/// value the header would come from, so a request missing `params.name` draws
/// its own defect rather than a spurious "missing header" here.
pub(super) fn mirrors(
    post: &Post<'_>,
    designated: &BTreeMap<String, Vec<Designation>>,
) -> Vec<Mirror> {
    let mut out = Vec::new();
    let Some(method) = post.method() else {
        return out;
    };
    out.push(Mirror {
        header: "mcp-method".to_owned(),
        label: "Mcp-Method".to_owned(),
        source: "method".to_owned(),
        value: method.to_owned(),
        encodable: false,
    });
    let params = post.payload.get("params");
    if let Some((_, field)) = NAME_SOURCED.iter().find(|(name, _)| *name == method)
        && let Some(value) = params
            .and_then(|params| params.get(*field))
            .and_then(Value::as_str)
    {
        out.push(Mirror {
            header: "mcp-name".to_owned(),
            label: "Mcp-Name".to_owned(),
            source: format!("params.{field}"),
            value: value.to_owned(),
            encodable: true,
        });
    }
    let tool = params
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str);
    let declared = (method == "tools/call")
        .then(|| tool.and_then(|tool| designated.get(tool)))
        .flatten();
    let arguments = params.and_then(|params| params.get("arguments"));
    for designation in declared.into_iter().flatten() {
        let Some(value) = arguments
            .and_then(|arguments| value_at(arguments, &designation.path))
            .and_then(header_text)
        else {
            continue;
        };
        out.push(Mirror {
            header: designation.header.clone(),
            label: format!("Mcp-Param-{}", designation.name),
            source: format!("params.arguments.{}", designation.path.join(".")),
            value,
            encodable: true,
        });
    }
    out
}
