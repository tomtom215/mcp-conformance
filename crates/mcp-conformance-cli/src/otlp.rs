// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Exchanges and findings as OpenTelemetry spans: `watch --otlp`'s encoding.
//!
//! **The conventions.** One span per [`Exchange`], named and attributed by the
//! OpenTelemetry semantic conventions for MCP — `docs/gen-ai/mcp.md` in
//! `open-telemetry/semantic-conventions-genai`, read at commit
//! [`SEMCONV_COMMIT`] (status: Development). Span name `{mcp.method.name}
//! {target}`, the target being `gen_ai.tool.name` or `gen_ai.prompt.name` when
//! there is one, else `{mcp.method.name}` alone; `mcp.method.name`,
//! `jsonrpc.request.id` (not for a notification), `mcp.session.id`,
//! `mcp.protocol.version`, `jsonrpc.protocol.version` (only when not `2.0`),
//! `network.transport` (`pipe` for stdio, `tcp` for HTTP) with
//! `network.protocol.name`/`version` for HTTP, `server.address`/`server.port`
//! for a client-initiated exchange over HTTP, `gen_ai.tool.name` with
//! `gen_ai.operation.name = execute_tool` for `tools/call`,
//! `gen_ai.prompt.name`, `mcp.resource.uri`, and on error `error.type`,
//! `rpc.response.status_code` and an `ERROR` status whose description is the
//! JSON-RPC error's message. No opt-in attribute (arguments, results, prompt
//! variables) is ever set: `watch` exports what the conventions require or
//! recommend, never payloads.
//!
//! **Span kind: `CLIENT`, for every exchange.** The conventions define the
//! client span as the initiator's view — "reported by the MCP client when it
//! initiates the request or notification or by the MCP server when server
//! initiates the operation", covering "the time to receive the response" — and
//! the server span as the receiver's processing. A tap on the wire sees the
//! request leave and the response come back, which is the initiator's
//! interval; it never sees the receiver's processing begin or end, so a
//! `SERVER` span would claim timing it does not have. So each exchange is one
//! `CLIENT` span, whoever initiated it, from the request's recording to the
//! response's. Accordingly every JSON-RPC error code is an error, as the
//! client span's note says (the server span's exempts `-32700`…`-32602` and
//! `-32002`).
//!
//! **Findings** are span events named [`FINDING_EVENT`], with the requirement
//! ID, level, outcome, revision, spec URL, check, detail and `seq` as
//! `mcp_conformance.*` attributes — this tool's own namespace, since the
//! conventions define none for conformance. A finding goes on the span of the
//! exchange holding its `seq` when that span has not been sent yet, else on
//! the latest unsent span: the exchange whose completion surfaced it.
//!
//! **Encoding.** OTLP/HTTP JSON, the protobuf JSON mapping with OTLP's two
//! departures: trace and span ids are lowercase hex, not base64, and enums are
//! integers. 64-bit integers (times, `intValue`) are decimal strings, as the
//! mapping writes them.

use std::time::{SystemTime, UNIX_EPOCH};

use mcp_conformance_core::trace::{Direction, TransportKind};
use serde::Serialize;

use crate::live::{Ending, Exchange, LiveFinding};

pub use spans::Spans;

mod spans;

/// The commit of `open-telemetry/semantic-conventions-genai` whose
/// `docs/gen-ai/mcp.md` this encoding follows (read 2026-10-13).
pub const SEMCONV_COMMIT: &str = "5293effb97f18ddb0602da884dadee6119d67679";

/// The span event name a finding is exported under.
pub const FINDING_EVENT: &str = "mcp_conformance.finding";

/// `SPAN_KIND_CLIENT`.
pub const SPAN_KIND_CLIENT: i32 = 3;
/// `STATUS_CODE_ERROR`.
pub const STATUS_CODE_ERROR: i32 = 2;

/// An `ExportTraceServiceRequest`: the body of a `POST /v1/traces`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    resource_spans: Vec<ResourceSpans>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSpans {
    resource: Resource,
    scope_spans: Vec<ScopeSpans>,
}

#[derive(Debug, Clone, Serialize)]
struct Resource {
    attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, Serialize)]
struct ScopeSpans {
    scope: Scope,
    spans: Vec<Span>,
}

#[derive(Debug, Clone, Serialize)]
struct Scope {
    name: &'static str,
    version: &'static str,
}

impl Request {
    /// One request carrying `spans`, from this tool as the reporting service.
    #[must_use]
    pub fn new(spans: Vec<Span>) -> Self {
        let version = env!("CARGO_PKG_VERSION");
        Self {
            resource_spans: vec![ResourceSpans {
                resource: Resource {
                    attributes: vec![
                        KeyValue::string("service.name", "mcp-conformance"),
                        KeyValue::string("service.version", version),
                    ],
                },
                scope_spans: vec![ScopeSpans {
                    scope: Scope {
                        name: "mcp-conformance watch",
                        version,
                    },
                    spans,
                }],
            }],
        }
    }
}

/// One span.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[allow(clippy::struct_field_names, reason = "OTLP's own field names")]
#[serde(rename_all = "camelCase")]
pub struct Span {
    trace_id: String,
    span_id: String,
    /// The span's name.
    pub name: String,
    kind: i32,
    start_time_unix_nano: String,
    end_time_unix_nano: String,
    attributes: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    events: Vec<Event>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<Status>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    time_unix_nano: String,
    name: &'static str,
    attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Status {
    code: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    message: String,
}

/// An attribute.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyValue {
    key: &'static str,
    value: AnyValue,
}

/// An attribute's value: OTLP's `AnyValue`, externally tagged as the JSON
/// mapping writes a `oneof` (`{"stringValue": "…"}`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum AnyValue {
    StringValue(String),
    /// An `int64`, written as a decimal string.
    IntValue(String),
}

impl KeyValue {
    fn string(key: &'static str, value: impl Into<String>) -> Self {
        Self {
            key,
            value: AnyValue::StringValue(value.into()),
        }
    }

    fn int(key: &'static str, value: impl Into<i64>) -> Self {
        Self {
            key,
            value: AnyValue::IntValue(value.into().to_string()),
        }
    }
}

/// Lowercase hex, as OTLP JSON writes ids.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Nanoseconds since the Unix epoch, as a decimal string.
fn nanos(at: SystemTime) -> String {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
        .to_string()
}

/// Where an HTTP session's server is, for `server.address` and `server.port`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Peer {
    /// The host, as the upstream URL names it.
    pub address: String,
    /// The port, the scheme's default when the URL names none.
    pub port: u16,
}

impl Peer {
    /// The server an upstream URL names.
    #[must_use]
    pub fn new(address: impl Into<String>, port: u16) -> Self {
        Self {
            address: address.into(),
            port,
        }
    }
}

/// The span for `exchange`, with ids `trace_id` and `span_id`.
#[must_use]
pub fn span(
    exchange: &Exchange,
    trace_id: [u8; 16],
    span_id: [u8; 8],
    peer: Option<&Peer>,
) -> Span {
    let target = exchange.tool.as_ref().or(exchange.prompt.as_ref());
    let name = target.map_or_else(
        || exchange.method.clone(),
        |target| format!("{} {target}", exchange.method),
    );
    let mut attributes = attributes(exchange, peer);
    let status = status(exchange, &mut attributes);
    Span {
        trace_id: hex(&trace_id),
        span_id: hex(&span_id),
        name,
        kind: SPAN_KIND_CLIENT,
        start_time_unix_nano: nanos(exchange.started),
        end_time_unix_nano: nanos(exchange.ended),
        attributes,
        events: Vec::new(),
        status,
    }
}

/// The conventions' attributes for `exchange`, but for its outcome's.
fn attributes(exchange: &Exchange, peer: Option<&Peer>) -> Vec<KeyValue> {
    let mut attributes = vec![KeyValue::string("mcp.method.name", &*exchange.method)];
    if let Some(id) = &exchange.id
        && id != "null"
    {
        // The id's string form: a string id without its JSON quotes.
        let id = serde_json::from_str::<String>(id).unwrap_or_else(|_| id.clone());
        attributes.push(KeyValue::string("jsonrpc.request.id", id));
    }
    if let Some(version) = &exchange.jsonrpc {
        attributes.push(KeyValue::string("jsonrpc.protocol.version", &**version));
    }
    if let Some(session) = &exchange.session_id {
        attributes.push(KeyValue::string("mcp.session.id", &**session));
    }
    if let Some(version) = &exchange.protocol_version {
        attributes.push(KeyValue::string("mcp.protocol.version", &**version));
    }
    match exchange.transport {
        TransportKind::StreamableHttp => {
            attributes.push(KeyValue::string("network.transport", "tcp"));
            attributes.push(KeyValue::string("network.protocol.name", "http"));
            attributes.push(KeyValue::string("network.protocol.version", "1.1"));
            if exchange.initiator == Direction::ClientToServer
                && let Some(peer) = peer
            {
                attributes.push(KeyValue::string("server.address", &*peer.address));
                attributes.push(KeyValue::int("server.port", peer.port));
            }
        }
        _ => attributes.push(KeyValue::string("network.transport", "pipe")),
    }
    if let Some(tool) = &exchange.tool {
        attributes.push(KeyValue::string("gen_ai.tool.name", &**tool));
        attributes.push(KeyValue::string("gen_ai.operation.name", "execute_tool"));
    }
    if let Some(prompt) = &exchange.prompt {
        attributes.push(KeyValue::string("gen_ai.prompt.name", &**prompt));
    }
    if let Some(uri) = &exchange.resource_uri {
        attributes.push(KeyValue::string("mcp.resource.uri", &**uri));
    }
    attributes
}

/// The span's status for how `exchange` ended, with the error attributes
/// added to `attributes`; `None` (unset) when it did not fail.
fn status(exchange: &Exchange, attributes: &mut Vec<KeyValue>) -> Option<Status> {
    match &exchange.ending {
        Ending::Answered {
            error: Some((code, message)),
            ..
        } => {
            // A JSON-RPC error without a code gets the conventions' fallback.
            attributes.push(KeyValue::string(
                "error.type",
                code.as_deref().unwrap_or("_OTHER"),
            ));
            if let Some(code) = code {
                attributes.push(KeyValue::string("rpc.response.status_code", &**code));
            }
            Some(Status {
                code: STATUS_CODE_ERROR,
                message: message.clone().unwrap_or_default(),
            })
        }
        Ending::Answered {
            tool_error: true, ..
        } if exchange.method == "tools/call" => {
            attributes.push(KeyValue::string("error.type", "tool_error"));
            Some(Status {
                code: STATUS_CODE_ERROR,
                message: String::new(),
            })
        }
        Ending::Unanswered => {
            attributes.push(KeyValue::string("error.type", "unanswered"));
            Some(Status {
                code: STATUS_CODE_ERROR,
                message: "the recording ended before a response".to_owned(),
            })
        }
        _ => None,
    }
}

impl Span {
    /// Adds `finding` as an event at `at`.
    pub fn add_finding(&mut self, finding: &LiveFinding, at: SystemTime) {
        let mut attributes = vec![
            KeyValue::string("mcp_conformance.requirement.id", &*finding.requirement),
            KeyValue::string("mcp_conformance.requirement.level", &*finding.level),
            KeyValue::string(
                "mcp_conformance.outcome",
                if finding.outcome == mcp_trace_validator::report::Outcome::Warn {
                    "warn"
                } else {
                    "fail"
                },
            ),
            KeyValue::string("mcp_conformance.revision", &*finding.revision),
            KeyValue::string("mcp_conformance.check", &*finding.check),
            KeyValue::string("mcp_conformance.detail", &*finding.detail),
        ];
        if let Some(url) = &finding.url {
            attributes.push(KeyValue::string("mcp_conformance.spec.url", &**url));
        }
        if let Some(seq) = finding.seq {
            attributes.push(KeyValue::int(
                "mcp_conformance.trace.seq",
                i64::try_from(seq).unwrap_or(i64::MAX),
            ));
        }
        self.events.push(Event {
            time_unix_nano: nanos(at),
            name: FINDING_EVENT,
            attributes,
        });
    }

    /// The findings it carries.
    #[must_use]
    pub const fn findings(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests;
