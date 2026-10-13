// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcpsnoop` captures as traces.
//!
//! An `mcpsnoop` capture is JSON Lines of *envelopes*: a `meta` envelope
//! (`raw.command` for stdio, `raw.target` for HTTP), then one per frame, in
//! the order mcpsnoop saw them, each with `direction` (`c2s`, `s2c`), `ts`,
//! and the frame verbatim as `raw`. Over HTTP an envelope also names the
//! exchange it rode (`conn_id`, `exchange`); a client frame carries the
//! request's routing headers (`mcp_protocol_version`, `mcp_method`,
//! `mcp_name`, `mcp_param_headers`) and `transport_headers` (`accept`,
//! `content_type`, `origin`); a server frame carries the response's `status`
//! and `transport_headers.content_type`; a batch's elements are separate
//! envelopes marked `batch`. The shape was read from captures mcpsnoop 0.26.1
//! wrote itself (`conformance/mcpsnoop/native/` in this repository, which
//! holds each session also as a trace this toolkit recorded at the same
//! time — the tests import one and compare it with the other), and from the
//! reverse conversion `cargo xtask mcpsnoop` makes.
//!
//! Each frame becomes one message event, a batch's elements one array again.
//! Over HTTP, the first frame of each exchange is preceded by what the
//! exchange shows of HTTP: a client frame by the request (`POST`, since a
//! client message is only ever sent in a POST body) with the headers
//! mcpsnoop kept, a server frame by the response status — and, when the
//! client sent nothing on that exchange first, by a request whose method is
//! left unstated, since the capture does not say whether it was a `GET`.
//! The list below is what a trace imported this way cannot hold.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent, TransportKind};
use serde_json::Value;

use super::{ImportError, Imported, write};

/// What an `mcpsnoop` capture does not record, on either transport.
pub const NOT_RECORDED: [&str; 2] = [
    "transport lifecycle (a process starting or exiting, a connection opening or failing): \
     mcpsnoop records frames only",
    "anything mcpsnoop did not record as a frame, such as a line that was not JSON",
];

/// What an `mcpsnoop` capture of an HTTP session does not record besides.
pub const NOT_RECORDED_HTTP: [&str; 4] = [
    "request headers other than Accept, Content-Type, Origin, MCP-Protocol-Version, Mcp-Method, \
     Mcp-Name and Mcp-Param-*: notably Host and Mcp-Session-Id",
    "response headers other than Content-Type: notably Mcp-Session-Id",
    "the method and the time of a request without a body (a GET stream, a DELETE): it is left \
     unstated, and the request is placed just before the first thing the server sent on it",
    "the transport headers of a batch POST",
];

fn field<'a>(envelope: &'a Value, name: &str) -> Option<&'a str> {
    envelope.get(name).and_then(Value::as_str)
}

/// The request headers a client envelope carries, in the trace's lowercase.
fn request_headers(envelope: &Value) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for (member, header) in [
        ("mcp_protocol_version", "mcp-protocol-version"),
        ("mcp_method", "mcp-method"),
        ("mcp_name", "mcp-name"),
    ] {
        if let Some(value) = field(envelope, member) {
            headers.insert(header.to_owned(), value.to_owned());
        }
    }
    for param in envelope
        .get("mcp_param_headers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(name), Some(value)) = (field(param, "name"), field(param, "value")) {
            headers.insert(name.to_ascii_lowercase(), value.to_owned());
        }
    }
    transport_headers(envelope, &mut headers);
    headers
}

fn transport_headers(envelope: &Value, headers: &mut BTreeMap<String, String>) {
    let transport = envelope.get("transport_headers");
    for (member, header) in [
        ("accept", "accept"),
        ("content_type", "content-type"),
        ("origin", "origin"),
    ] {
        if let Some(value) = transport
            .and_then(|t| t.get(member))
            .and_then(Value::as_str)
        {
            headers.insert(header.to_owned(), value.to_owned());
        }
    }
}

/// An HTTP exchange: its connection and number.
type Exchange = (String, u64);

/// A batch being collected: its exchange, direction, elements and the first
/// element's time.
type Batch = (Exchange, Direction, Vec<Value>, Option<String>);

/// The trace being built.
struct Builder {
    transport: TransportKind,
    events: Vec<TraceEvent>,
    /// Exchanges already opened.
    opened: BTreeSet<Exchange>,
    /// Exchanges whose response status is already recorded.
    answered: BTreeSet<Exchange>,
    /// The batch being collected: its exchange, direction and elements.
    batch: Option<Batch>,
}

impl Builder {
    fn push(&mut self, direction: Direction, body: EventBody, ts: Option<&str>) {
        let mut event = TraceEvent::new(0, direction, self.transport, body);
        event.ts = ts.map(ToOwned::to_owned);
        self.events.push(event);
    }

    fn flush_batch(&mut self) {
        if let Some((_, direction, parts, ts)) = self.batch.take() {
            let body = EventBody::Message {
                payload: Value::Array(parts),
            };
            self.push(direction, body, ts.as_deref());
        }
    }

    fn frame(&mut self, envelope: &Value) {
        let direction = match field(envelope, "direction") {
            Some("c2s") => Direction::ClientToServer,
            Some("s2c") => Direction::ServerToClient,
            _ => return,
        };
        let ts = field(envelope, "ts");
        let exchange = match (
            field(envelope, "conn_id"),
            envelope.get("exchange").and_then(Value::as_u64),
        ) {
            (Some(conn), Some(number)) if self.transport == TransportKind::StreamableHttp => {
                Some((conn.to_owned(), number))
            }
            _ => None,
        };
        let batched = envelope.get("batch").and_then(Value::as_bool) == Some(true);
        let continues = matches!(
            (&self.batch, &exchange),
            (Some((open, open_direction, ..)), Some(this)) if batched && open == this && *open_direction == direction
        );
        if !continues {
            self.flush_batch();
        }
        if let Some(key) = exchange.clone() {
            self.http(envelope, direction, key, ts);
        }
        let Some(raw) = envelope.get("raw").cloned() else {
            return;
        };
        match (batched, exchange) {
            (true, Some(key)) => {
                if let Some((_, _, parts, _)) = &mut self.batch {
                    parts.push(raw);
                } else {
                    self.batch = Some((key, direction, vec![raw], ts.map(ToOwned::to_owned)));
                }
            }
            _ => self.push(direction, EventBody::Message { payload: raw }, ts),
        }
    }

    /// What the frame's exchange shows of HTTP, the first time it shows it.
    fn http(&mut self, envelope: &Value, direction: Direction, key: Exchange, ts: Option<&str>) {
        let fresh = self.opened.insert(key.clone());
        if direction == Direction::ClientToServer {
            if fresh {
                let body = http_event(Some("POST"), None, request_headers(envelope));
                self.push(direction, body, ts);
            }
            return;
        }
        if fresh {
            let body = http_event(None, None, BTreeMap::new());
            self.push(Direction::ClientToServer, body, None);
        }
        if let Some(status) = envelope.get("status").and_then(Value::as_u64)
            && self.answered.insert(key)
        {
            let mut headers = BTreeMap::new();
            transport_headers(envelope, &mut headers);
            headers.retain(|name, _| name == "content-type");
            let status = u16::try_from(status).ok();
            self.push(direction, http_event(None, status, headers), ts);
        }
    }
}

fn http_event(
    method: Option<&str>,
    status: Option<u16>,
    headers: BTreeMap<String, String>,
) -> EventBody {
    EventBody::Http {
        method: method.map(ToOwned::to_owned),
        status,
        headers,
    }
}

/// Imports an `mcpsnoop` capture.
///
/// ```
/// use mcp_conformance_cli::import::mcpsnoop;
///
/// let capture = r#"{"session_id":"s","seq":1,"ts":"2026-10-13T10:00:00Z","direction":"meta","transport":"http","raw":{"command":null,"target":"http://127.0.0.1:3000/mcp"}}
/// {"session_id":"s","seq":2,"ts":"2026-10-13T10:00:00.001Z","direction":"c2s","transport":"http","raw":{"jsonrpc":"2.0","id":1,"method":"ping"},"conn_id":"c","exchange":1,"transport_headers":{"accept":"application/json, text/event-stream","content_type":"application/json"}}
/// {"session_id":"s","seq":3,"ts":"2026-10-13T10:00:00.002Z","direction":"s2c","transport":"http","raw":{"jsonrpc":"2.0","id":1,"result":{}},"conn_id":"c","exchange":1,"status":200,"transport_headers":{"content_type":"application/json"}}
/// "#;
/// let imported = mcpsnoop::import(capture)?;
/// let kinds: Vec<&str> = imported.trace.lines()
///     .map(|line| if line.contains(r#""kind":"http""#) { "http" } else { "message" })
///     .collect();
/// assert_eq!(kinds, ["http", "message", "http", "message"]);
/// assert!(imported.not_recorded.iter().any(|gap| gap.contains("Mcp-Session-Id")));
/// # Ok::<(), mcp_conformance_cli::import::ImportError>(())
/// ```
///
/// # Errors
///
/// [`ImportError`] when a line is not JSON, the first is not a `meta`
/// envelope, or the capture names a transport other than `stdio` or `http`.
pub fn import(text: &str) -> Result<Imported, ImportError> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    let parse = |(number, line): (usize, &str)| -> Result<Value, ImportError> {
        serde_json::from_str(line)
            .map_err(|error| ImportError(format!("line {} is not JSON: {error}", number + 1)))
    };
    let meta = lines
        .next()
        .map(parse)
        .transpose()?
        .filter(|meta| field(meta, "direction") == Some("meta"))
        .ok_or_else(|| ImportError("an mcpsnoop capture starts with a meta envelope".to_owned()))?;
    let transport = match field(&meta, "transport") {
        Some("stdio") => TransportKind::Stdio,
        Some("http") => TransportKind::StreamableHttp,
        other => {
            return Err(ImportError(format!(
                "the capture's transport is {other:?}; mcpsnoop records stdio or http"
            )));
        }
    };
    let mut builder = Builder {
        transport,
        events: Vec::new(),
        opened: BTreeSet::new(),
        answered: BTreeSet::new(),
        batch: None,
    };
    for line in lines {
        builder.frame(&parse(line)?);
    }
    builder.flush_batch();
    let mut not_recorded: Vec<String> = NOT_RECORDED.iter().map(|gap| (*gap).to_owned()).collect();
    if transport == TransportKind::StreamableHttp {
        not_recorded.extend(NOT_RECORDED_HTTP.iter().map(|gap| (*gap).to_owned()));
    }
    Ok(Imported {
        trace: write(builder.events),
        not_recorded,
        recorded_at: field(&meta, "ts").map(ToOwned::to_owned),
    })
}
