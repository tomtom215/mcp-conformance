// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The recorded-trace event schema.
//!
//! A *trace* is an ordered record of everything observable about one protocol session:
//! the JSON-RPC messages in both directions, plus the transport-level events real
//! requirements attach to (connections opening and closing; HTTP exchanges with their
//! status and security-relevant headers). Traces are exchanged as JSON Lines — one
//! [`TraceEvent`] object per line — because that format streams, diffs, and survives
//! truncation legibly.
//!
//! Design rules:
//!
//! - `seq` is assigned at capture time and is the only ordering authority. Validators
//!   never infer order from anything else.
//! - Timestamps are optional, informational metadata; no check may depend on them
//!   (determinism would die).
//! - Capture tooling redacts credential-bearing headers by default; the schema carries
//!   whatever the capturer recorded and adds no secrets of its own.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Deserializes an HTTP-headers map with field names normalized to lowercase.
///
/// HTTP field names are case-insensitive (RFC 9110 §5.1); the checks compare
/// against lowercase names, so normalizing here keeps a trace's casing from
/// changing a verdict. If two differently-cased spellings of one field appear
/// (a malformed trace), they collapse to one lowercase key — last-in-sorted-
/// order wins, which is deterministic; conflicting duplicates are themselves a
/// capture defect.
fn deserialize_lowercase_header_keys<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = BTreeMap::<String, String>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect())
}

/// Deserializes an HTTP request method with the token normalized to uppercase.
///
/// RFC 9110 §9.1 makes method tokens case-sensitive and the registered methods
/// are all uppercase, so a capture recording `post` is describing the same
/// request a capture recording `POST` describes — it is the capturer's casing,
/// not the wire's. Normalizing here means a lowercasing capturer cannot make a
/// method-gated clause silently stop being judged, which is the failure mode
/// the header normalization above exists to prevent, one field over.
fn deserialize_uppercase_method<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.map(|method| method.to_ascii_uppercase()))
}

/// Which party emitted an event.
///
/// Deliberately *not* `#[non_exhaustive]`: a protocol exchange has exactly two ends,
/// so downstream code may match this exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    /// Client → server.
    ClientToServer,
    /// Server → client.
    ServerToClient,
}

/// The transport a session ran over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TransportKind {
    /// stdio transport: newline-delimited messages over a child process's stdin/stdout.
    Stdio,
    /// Streamable HTTP transport: HTTP POST/GET with optional SSE streaming.
    StreamableHttp,
}

/// Transport lifecycle moments that checks may need to anchor to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum LifecycleEvent {
    /// The transport became usable (process spawned / connection established).
    TransportOpen,
    /// The transport closed in an orderly way.
    TransportClose,
    /// The transport failed or was torn down abnormally.
    TransportAbort,
}

/// The event-specific portion of a [`TraceEvent`], discriminated by a `kind` field on
/// the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum EventBody {
    /// A JSON-RPC message, stored as parsed JSON.
    Message {
        /// The message payload as parsed JSON: every value as captured, but not
        /// its formatting (whitespace, member order, the spelling of a number).
        payload: Value,
    },
    /// An HTTP-level observation (Streamable HTTP transport only).
    Http {
        /// The request method, when this event records a client request.
        ///
        /// Streamable HTTP gives the client three request forms and binds
        /// different obligations to each — `POST` carries messages, `GET`
        /// opens a standalone stream, `DELETE` terminates a session — so a
        /// clause addressed to one of them cannot be judged from an exchange
        /// that does not say which it was. Absent (the shape every capture
        /// predating this field has, and every server-side response event),
        /// method-gated checks report the clause *not observed* rather than
        /// guessing: a recording that cannot tell a POST from a DELETE cannot
        /// evidence a POST-only MUST, and must not fail a DELETE with one.
        /// Normalized to uppercase on deserialization.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_uppercase_method"
        )]
        method: Option<String>,
        /// Response status, when this event records a response.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
        /// Headers relevant to conformance. HTTP field names are
        /// case-insensitive (RFC 9110 §5.1), so keys are normalized to
        /// lowercase on deserialization — the contract the transport checks
        /// rely on when they look a header up by its lowercase name. A trace
        /// recording on-the-wire casing (`Mcp-Session-Id`) is therefore judged
        /// the same as one already lowercased; a lossy capture cannot hide a
        /// bad header behind its casing. Capture tooling redacts
        /// credential-bearing headers by default.
        #[serde(
            default,
            skip_serializing_if = "BTreeMap::is_empty",
            deserialize_with = "deserialize_lowercase_header_keys"
        )]
        headers: BTreeMap<String, String>,
        /// The request's path, on a request to a well-known URI and on the
        /// response to it — and on no other event.
        ///
        /// A recorder keeps a path only when [`records_path`] says so: under
        /// `/.well-known/`, which RFC 8615 reserves for public metadata. The
        /// MCP endpoint's own path is never recorded, and no query ever is.
        /// An event that carries one is not MCP traffic: it is a fetch of a
        /// document beside the endpoint, judged only as that.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// The JSON document a well-known URI answered with, on the response
        /// event (the one carrying `status` and `path`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        document: Option<Value>,
    },
    /// A transport lifecycle moment.
    Lifecycle {
        /// Which moment occurred.
        event: LifecycleEvent,
    },
}

/// One captured event in a protocol trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TraceEvent {
    /// Capture-assigned sequence number; the total order within the trace.
    pub seq: u64,
    /// Which party emitted the event.
    pub direction: Direction,
    /// The transport the session ran over.
    pub transport: TransportKind,
    /// The event-specific body (`kind` discriminated).
    #[serde(flatten)]
    pub body: EventBody,
    /// Optional informational timestamp (RFC 3339). Never consulted by checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

impl TraceEvent {
    /// Builds an event with no timestamp — the constructor capture tooling
    /// uses (the struct is `#[non_exhaustive]`, so literals only work inside
    /// this crate). `ts` stays settable afterwards; checks never read it.
    #[must_use]
    pub const fn new(
        seq: u64,
        direction: Direction,
        transport: TransportKind,
        body: EventBody,
    ) -> Self {
        Self {
            seq,
            direction,
            transport,
            body,
            ts: None,
        }
    }

    /// The JSON-RPC payload, when this event is a message.
    #[must_use]
    pub const fn message_payload(&self) -> Option<&Value> {
        match &self.body {
            EventBody::Message { payload } => Some(payload),
            EventBody::Http { .. } | EventBody::Lifecycle { .. } => None,
        }
    }
}

/// The exact set of HTTP header names (lowercase) capture tooling records into traces.
///
/// This is the recording allowlist, public so a consumer can verify precisely
/// what a capture keeps: everything absent from it — notably `authorization`
/// and `cookie` — is never written to a trace ([05-security-model.md]'s
/// redaction-by-construction posture). The everything server's tap and
/// `mcp-trace-capture` both record exactly this set.
///
/// [05-security-model.md]: https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/05-security-model.md
pub const RECORDED_HEADERS: [&str; 11] = [
    "host",
    "origin",
    "accept",
    "content-type",
    "mcp-session-id",
    "mcp-protocol-version",
    "last-event-id",
    // SEP-2243's request metadata headers and SEP-2570's streaming hint, all
    // read by `2026-07-28` checks. Absent from this list until 2026-08-17,
    // which made the tap report a conforming exchange as a violating one: the
    // client sent `Mcp-Method`, the recording dropped it, and
    // `transport.request-metadata-headers` reported the clause it proves.
    "mcp-method",
    "mcp-name",
    "x-accel-buffering",
    // A protected resource's challenge (RFC 9110 §11.6.1): its scheme and
    // parameters — `scope`, `error`, `resource_metadata` — are what the
    // authorization clauses that a recording can judge read. It is the server
    // asking for a credential, never one: the credential travels in the
    // client's `Authorization`, which stays off this list. Absent until
    // 2026-10-13, so a 401 recorded before then carries its status alone.
    "www-authenticate",
];

/// The one `Authorization` value a recorder keeps: `check`'s probe.
///
/// Not a credential: a public constant no authorization server issued, kept
/// only as the field's whole value, so a recording says nothing about any
/// other request's credential.
pub const PROBE_AUTHORIZATION: &str = "Bearer mcp-conformance-probe-invalid-token";

/// Whether a recorder keeps the header `name` (lowercase) with `value`: on
/// either allowlist, or `authorization` carrying [`PROBE_AUTHORIZATION`].
#[must_use]
pub fn records_header(name: &str, value: &str) -> bool {
    RECORDED_HEADERS.contains(&name)
        || RECORDED_HEADER_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || (name == "authorization" && value == PROBE_AUTHORIZATION)
}

/// The only request paths a recorder keeps start with this: RFC 8615's
/// well-known URIs, where public metadata lives.
pub const RECORDED_PATH_PREFIX: &str = "/.well-known/";

/// Whether a recorder keeps the request path `path` (no query): only one
/// under [`RECORDED_PATH_PREFIX`] that names something there.
#[must_use]
pub fn records_path(path: &str) -> bool {
    path.len() > RECORDED_PATH_PREFIX.len() && path.starts_with(RECORDED_PATH_PREFIX)
}

/// Header-name prefixes recorded in addition to [`RECORDED_HEADERS`].
///
/// SEP-2243 lets a tool designate an argument for a header of its own naming
/// (`x-mcp-header` in the tool's `inputSchema`), which the client sends as
/// `Mcp-Param-<name>`. The set of such names is defined by whatever server the
/// capture sits in front of, so it cannot be enumerated here — a prefix is the
/// only allowlist shape that can cover it.
///
/// This does not widen what a recording exposes. Every `Mcp-Param-*` value is
/// by definition a copy of an argument in the `tools/call` body a capture
/// already records verbatim; the prefix cannot match `authorization` or
/// `cookie`, and no other header may use it.
pub const RECORDED_HEADER_PREFIXES: [&str; 1] = ["mcp-param-"];

/// The JSON Schema (draft 2020-12) of one trace record, for producers written
/// without this crate: a recorder in any language can check its output against it.
///
/// It states every rule the reader applies to a single record, and a test runs
/// both over the whole corpus and one violation of each rule. It cannot state
/// the document-level rules (one record per line, no blank lines or byte-order
/// mark, `seq` strictly increasing), and it accepts `1.0` where the reader wants
/// `1`; its description says so.
pub const EVENT_JSON_SCHEMA: &str = include_str!("../schema/trace-event.schema.json");

/// The largest message a recorder captures by default, in bytes (64 MiB).
///
/// One definition for the whole toolkit, beside the format it bounds: a trace a
/// recorder writes with its defaults must be one a reader accepts with its own.
/// The two limits lived in different crates until 2026-09-25, at 64 MiB and
/// 1 MiB, so a session carrying one large tool result — a base64 image is
/// enough — was recorded faithfully and then refused whole as malformed.
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// How much longer than its message limit a recorder lets a trace line be: room
/// for the event around the payload (1 MiB).
///
/// Room, not a bound on growth. A recorder stores the payload re-serialized,
/// and although that drops whitespace and never lengthens a string, a number
/// can grow: `9e15` is written back as `9000000000000000.0`, so a message of
/// such numbers nearly quadruples. `mcp-trace-capture` therefore checks the
/// line it writes against `message limit + LINE_ENVELOPE_BYTES` and counts one
/// that does not fit as oversized, which makes the bound hold by construction
/// rather than by an argument about what payloads look like.
pub const LINE_ENVELOPE_BYTES: usize = 1024 * 1024;

/// The longest trace line a reader accepts by default, in bytes: exactly the
/// longest line a recorder writes with its default message limit.
pub const DEFAULT_MAX_LINE_BYTES: usize = DEFAULT_MAX_MESSAGE_BYTES + LINE_ENVELOPE_BYTES;

#[cfg(test)]
mod tests;
