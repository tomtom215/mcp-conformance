// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One of our traces, rewritten as the capture `mcpsnoop` records itself.
//!
//! mcpsnoop's capture is JSON Lines of *envelopes*: a `meta` envelope, then
//! one per frame, carrying the frame verbatim as `raw` and — on HTTP — the
//! exchange it rode (`conn_id`, `exchange`), the request's routing headers
//! (`mcp_method`, `mcp_name`, `mcp_protocol_version`, `mcp_param_headers`,
//! `transport_headers.accept`/`content_type`/`origin`) and the response's
//! `status` and `content_type`. The shape was read off captures 0.26.1 wrote
//! itself; `native/` beside the results holds the evidence.
//!
//! The rule is that **conversion invents nothing a verdict reads**:
//!
//! - **stdio** converts losslessly. Lifecycle events have no counterpart and
//!   are dropped; mcpsnoop reads no transport moment.
//! - **HTTP** context is attached only where the trace's order settles it, by
//!   the rules the validator's own framing applies
//!   (`mcp-trace-validator`'s `context/framing.rs`): a POST's headers go on
//!   its body only when no other POST overlapped it; a status goes on the
//!   exchange it ended only when that exchange alone awaited one; a response
//!   rides the exchange of the request its id names. A run of overlapping
//!   exchanges that all drew the same status and `Content-Type` gets it — any
//!   assignment would read the same. Everything else is left without HTTP
//!   context, and is reported as *unframed* so a finding that reads such
//!   context is set aside rather than counted (`classify.rs`).
//! - A server notification or request is never given an exchange: a trace
//!   does not record which stream carried it.
//! - Three fields are fixed rather than recorded, because the trace format
//!   holds no such fact: one `conn_id` for the whole session (mcpsnoop pairs a
//!   server request with its answer within one connection — the spec scopes
//!   ids to the session), synthetic timestamps one millisecond apart (no rule
//!   compared here reads time), and a constant session label.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::trace::{
    Direction, EventBody, LifecycleEvent, TraceEvent, TransportKind,
};
use serde_json::{Map, Value, json};

/// The one connection every converted HTTP frame claims.
pub(crate) const CONNECTION: &str = "mcp-conformance";

/// The capture mcpsnoop is given, with each line's origin.
#[derive(Debug)]
pub(crate) struct Capture {
    /// The envelopes, one JSON object per line, `meta` first.
    pub lines: Vec<Value>,
    /// What each line came from, index for index.
    pub frames: Vec<Frame>,
}

/// Where one envelope came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Frame {
    /// The trace event's `seq`; `None` for the `meta` envelope.
    pub seq: Option<u64>,
    /// Whether the envelope carries all the HTTP context the frame had on the
    /// wire. Always `true` on stdio, which has none.
    pub framed: bool,
}

/// A status and its media type, as an exchange's response carried them.
type Status = (u16, Option<String>);

/// One HTTP exchange as far as the trace tells it.
#[derive(Debug, Default)]
struct Exchange<'a> {
    headers: Option<&'a BTreeMap<String, String>>,
    has_body: bool,
    paired: bool,
    status: Option<Status>,
}

/// Overlapping exchanges whose statuses the order cannot tell apart.
#[derive(Debug, Default)]
struct Window {
    members: Vec<usize>,
    statuses: Vec<Option<Status>>,
    left: usize,
}

/// One envelope before the exchanges' statuses are all known.
#[derive(Debug)]
enum Draft {
    Status {
        seq: u64,
        exchange: usize,
    },
    Message {
        seq: u64,
        client: bool,
        raw: Value,
        batch: bool,
        exchange: Option<usize>,
    },
}

#[derive(Default)]
struct Builder<'a> {
    /// Streamable HTTP; on stdio a batch stays the one frame the shim records.
    http: bool,
    exchanges: Vec<Exchange<'a>>,
    awaiting: Vec<usize>,
    open_bodies: usize,
    overlapped: bool,
    window: Option<Window>,
    by_request: BTreeMap<String, Option<usize>>,
    /// The exchange the status just recorded was attributed to, and whether
    /// no request awaited it, until a server message follows.
    fresh: Option<(usize, bool)>,
    drafts: Vec<Draft>,
}

/// A message's non-null `id`, as canonical text.
fn id_text(payload: &Value) -> Option<String> {
    payload
        .get("id")
        .filter(|id| !id.is_null())
        .map(ToString::to_string)
}

fn is_response(payload: &Value) -> bool {
    payload.get("method").is_none()
        && (payload.get("result").is_some() || payload.get("error").is_some())
}

impl<'a> Builder<'a> {
    fn step(&mut self, event: &'a TraceEvent) {
        let client = event.direction == Direction::ClientToServer;
        match (&event.body, client) {
            (
                EventBody::Http {
                    method, headers, ..
                },
                true,
            ) => {
                let has_body = !matches!(method.as_deref(), Some("GET" | "DELETE"));
                if has_body {
                    self.open_bodies += 1;
                    self.overlapped |= self.open_bodies > 1;
                }
                self.exchanges.push(Exchange {
                    headers: Some(headers),
                    has_body,
                    ..Exchange::default()
                });
                let index = self.exchanges.len() - 1;
                self.awaiting.push(index);
                if let Some(window) = &mut self.window {
                    window.members.push(index);
                    window.left += 1;
                }
                self.fresh = None;
            }
            (
                EventBody::Http {
                    status: Some(code),
                    headers,
                    ..
                },
                false,
            ) => {
                let status = (*code, headers.get("content-type").cloned());
                self.ended(event.seq, Some(status));
            }
            (
                EventBody::Lifecycle {
                    event: LifecycleEvent::TransportAbort,
                },
                false,
            ) => {
                self.ended(event.seq, None);
            }
            (EventBody::Message { payload }, true) => self.client_message(event.seq, payload),
            (EventBody::Message { payload }, false) => self.server_message(event.seq, payload),
            _ => self.fresh = None,
        }
    }

    /// A status (or an abort standing in for one) ends one exchange.
    fn ended(&mut self, seq: u64, status: Option<Status>) {
        self.fresh = None;
        if self.awaiting.is_empty() {
            if let Some(status) = status {
                self.exchanges.push(Exchange {
                    status: Some(status),
                    ..Exchange::default()
                });
                let index = self.exchanges.len() - 1;
                self.drafts.push(Draft::Status {
                    seq,
                    exchange: index,
                });
                self.fresh = Some((index, true));
            }
            return;
        }
        if self.window.is_none() && self.awaiting.len() > 1 {
            self.window = Some(Window {
                members: self.awaiting.clone(),
                statuses: Vec::new(),
                left: self.awaiting.len(),
            });
        }
        let index = self.awaiting.remove(0);
        let Some(window) = &mut self.window else {
            if status.is_some() {
                self.exchanges[index].status = status;
                self.drafts.push(Draft::Status {
                    seq,
                    exchange: index,
                });
                self.fresh = Some((index, false));
            }
            return;
        };
        window.statuses.push(status);
        window.left -= 1;
        if window.left == 0 {
            let window = self.window.take().unwrap_or_default();
            let first = window.statuses.first().cloned().flatten();
            if first.is_some() && window.statuses.iter().all(|status| *status == first) {
                for member in window.members {
                    self.exchanges[member].status.clone_from(&first);
                }
            }
        }
    }

    fn client_message(&mut self, seq: u64, payload: &Value) {
        self.fresh = None;
        let request = payload.get("method").and_then(|_| id_text(payload));
        let mut exchange = None;
        if self.open_bodies > 0 {
            let certain = self.open_bodies == 1 && !self.overlapped;
            self.open_bodies -= 1;
            if self.open_bodies == 0 {
                self.overlapped = false;
            }
            if certain {
                exchange = self.awaiting.iter().rev().copied().find(|&index| {
                    let candidate = &self.exchanges[index];
                    candidate.has_body && !candidate.paired
                });
                if let Some(index) = exchange {
                    self.exchanges[index].paired = true;
                }
            }
        }
        if let Some(id) = request {
            self.by_request.insert(id, exchange);
        }
        self.push_parts(seq, true, payload, |_| exchange);
    }

    fn server_message(&mut self, seq: u64, payload: &Value) {
        let fresh = self.fresh.take();
        let by_request = &self.by_request;
        let exchange_for = |part: &Value| {
            if !is_response(part) {
                return None; // which stream carried it, the trace does not say
            }
            match (id_text(part), fresh) {
                (Some(id), fresh) => by_request.get(&id).copied().flatten().or_else(|| {
                    fresh.and_then(|(index, unaccounted)| unaccounted.then_some(index))
                }),
                (None, Some((index, false))) => Some(index),
                (None, _) => None,
            }
        };
        let exchanges: Vec<Option<usize>> = match payload {
            Value::Array(parts) => parts.iter().map(exchange_for).collect(),
            part => vec![exchange_for(part)],
        };
        let mut next = exchanges.into_iter();
        self.push_parts(seq, false, payload, |_| next.next().flatten());
    }

    /// One draft per message, or per element of a batch.
    fn push_parts(
        &mut self,
        seq: u64,
        client: bool,
        payload: &Value,
        mut exchange: impl FnMut(&Value) -> Option<usize>,
    ) {
        let (parts, batch): (Vec<&Value>, bool) = match payload {
            Value::Array(parts) if self.http => (parts.iter().collect(), true),
            part => (vec![part], false),
        };
        for part in parts {
            let exchange = exchange(part);
            self.drafts.push(Draft::Message {
                seq,
                client,
                raw: part.clone(),
                batch,
                exchange,
            });
        }
    }
}

/// Converts one trace; `name` labels the session and its `meta` envelope.
pub(crate) fn convert(name: &str, events: &[TraceEvent]) -> Capture {
    let http = events
        .iter()
        .any(|event| event.transport == TransportKind::StreamableHttp);
    let mut builder = Builder {
        http,
        ..Builder::default()
    };
    for event in events {
        builder.step(event);
    }
    let Builder {
        exchanges, drafts, ..
    } = builder;
    let answered: BTreeSet<usize> = drafts
        .iter()
        .filter_map(|draft| match draft {
            Draft::Message {
                client: false,
                exchange,
                ..
            } => *exchange,
            _ => None,
        })
        .collect();
    let mut meta = Map::new();
    meta.insert("direction".to_owned(), json!("meta"));
    meta.insert("raw".to_owned(), json!({ "command": null, "target": name }));
    let first = (
        meta,
        Frame {
            seq: None,
            framed: true,
        },
    );
    let envelopes = std::iter::once(first).chain(
        drafts
            .into_iter()
            .filter_map(|draft| envelope(draft, &exchanges, &answered, http)),
    );
    let mut capture = Capture {
        lines: Vec::new(),
        frames: Vec::new(),
    };
    for (mut envelope, frame) in envelopes {
        let n = capture.lines.len() + 1;
        envelope.insert(
            "session_id".to_owned(),
            json!(format!("mcp-conformance:{name}")),
        );
        envelope.insert("server_label".to_owned(), json!("mcp-conformance"));
        envelope.insert("seq".to_owned(), json!(n));
        envelope.insert("ts".to_owned(), json!(timestamp(n)));
        envelope.insert(
            "transport".to_owned(),
            json!(if http { "http" } else { "stdio" }),
        );
        capture.lines.push(Value::Object(envelope));
        capture.frames.push(frame);
    }
    capture
}

/// One draft as an envelope, or `None` for a status mcpsnoop would not record
/// on its own.
fn envelope(
    draft: Draft,
    exchanges: &[Exchange<'_>],
    answered: &BTreeSet<usize>,
    http: bool,
) -> Option<(Map<String, Value>, Frame)> {
    let mut envelope = Map::new();
    match draft {
        Draft::Status { seq, exchange } => {
            // A status reaches mcpsnoop on the frames it framed; alone only
            // when its exchange carried no message back.
            let status = exchanges[exchange].status.as_ref()?;
            if answered.contains(&exchange) {
                return None;
            }
            envelope.insert("direction".to_owned(), json!("s2c"));
            envelope.insert("conn_id".to_owned(), json!(CONNECTION));
            envelope.insert("exchange".to_owned(), json!(exchange + 1));
            response_fields(status, &mut envelope);
            Some((
                envelope,
                Frame {
                    seq: Some(seq),
                    framed: true,
                },
            ))
        }
        Draft::Message {
            seq,
            client,
            raw,
            batch,
            exchange,
        } => {
            let direction = if client { "c2s" } else { "s2c" };
            envelope.insert("direction".to_owned(), json!(direction));
            envelope.insert("raw".to_owned(), raw);
            let known = exchange.map(|index| (index, &exchanges[index]));
            let framed = !http || http_message(&mut envelope, client, batch, known);
            Some((
                envelope,
                Frame {
                    seq: Some(seq),
                    framed,
                },
            ))
        }
    }
}

mod fields;

use fields::{http_message, response_fields, timestamp};

#[cfg(test)]
mod tests;
