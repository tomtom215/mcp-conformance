// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which HTTP exchange each recorded message belongs to — and when a trace
//! cannot say.
//!
//! The capture format records a request's `http` event and then the message its
//! body carried, and a response's status and then the message(s) it framed. It
//! carries no correlation between them, so the pairing is by order, and order is
//! only evidence while one exchange is open at a time. The capture tool awaits
//! between recording a request's headers and its body, so two concurrent POSTs
//! can land as `http(A) http(B) msg(A) msg(B)` — and pairing "the next client
//! message" put B's headers on A's body. Recorded off the official Python SDK
//! with concurrent calls, that convicted a conforming client of TRAN-058 and a
//! conforming server of TRAN-097/TRAN-100 on every interleaving.
//!
//! The rule here is that **an ambiguous pairing is no pairing**. A POST is
//! paired with its body only when no other POST was waiting for one; a response
//! status is attributed to a request only when that request's POST was the one
//! exchange awaiting a status. Everything the order cannot settle is left out,
//! which can only make a clause *not observed*, never convict.
//!
//! What is attributed, and how:
//!
//! - **POSTs** ([`Framing::posts`]): each client `http` event that is not a
//!   `GET` or `DELETE` (those carry no body) opens a slot. A client message
//!   fills the open slot when exactly one is open and no second one opened while
//!   it was; a run of overlapping POSTs taints every slot in it until all have
//!   drained.
//! - **Statuses** ([`Framing::status_for`]): every client `http` event awaits
//!   one response status. A status arriving while exactly one exchange awaits is
//!   that exchange's; with several awaiting, every one of them is left
//!   unattributed. A response message then rides the status of the request its
//!   `id` names — not merely the latest status before it, which on concurrent
//!   streams is another request's.
//!   A status no recorded exchange awaited — a hand-built or partial trace with
//!   response framing only — frames the message immediately after it.
//! - **Exchanges** ([`Framing::exchanges`]): each response status attributed
//!   to one client `http` event — its verb, the message it carried when that
//!   pairing is certain, and the response's status and headers. A status no
//!   recorded exchange awaited is listed too, with no request side, noting
//!   whether a JSON-RPC response followed it.
//! - **Unreadable ids** ([`Framing::unidentified_answers`]): a response whose
//!   `id` is null is tied to a request only when it is the first message after
//!   a status that is unambiguously that request's — the one case where "the
//!   ID could not be read" can be checked against the request the trace holds.

use std::collections::BTreeMap;

use mcp_conformance_core::trace::{
    Direction, EventBody, LifecycleEvent, TraceEvent, TransportKind,
};
use serde_json::Value;

use super::Post;
use crate::context::TraceContext;

/// One client `http` event awaiting its response status.
#[derive(Debug, Clone)]
struct Slot<'a> {
    /// The `seq` of the client `http` event.
    seq: u64,
    /// The request's HTTP method, when recorded.
    method: Option<&'a str>,
    /// The request's headers.
    headers: &'a BTreeMap<String, String>,
    /// Whether this exchange carries a body to pair (not a `GET` or `DELETE`).
    has_body: bool,
    /// The paired request message: its `seq` and `id` text, once paired.
    request: Option<(u64, Option<String>)>,
    /// The paired message itself, once paired.
    carried: Option<&'a Value>,
    /// Whether a status arrived while this slot and another both awaited one,
    /// so that a later status cannot be told apart as this slot's.
    tainted: bool,
}

/// One HTTP exchange whose response status the trace attributes.
#[derive(Debug, Clone, Copy)]
pub(in crate::checks) struct Exchange<'a> {
    /// The client `http` event: its `seq`, method and headers. `None` for a
    /// status no recorded exchange awaited.
    pub request: Option<(u64, Option<&'a str>, &'a BTreeMap<String, String>)>,
    /// The message the request carried, when the pairing is certain.
    pub carried: Option<&'a Value>,
    /// The response status event's `seq`.
    pub status_seq: u64,
    /// The response status.
    pub status: u16,
    /// The response headers.
    pub headers: &'a BTreeMap<String, String>,
    /// Whether a JSON-RPC response was the first server message after the status.
    pub framed_response: bool,
}

impl Exchange<'_> {
    /// The request's HTTP method, when the exchange has a recorded request.
    pub(in crate::checks) fn method(&self) -> Option<&str> {
        self.request.and_then(|(_, method, _)| method)
    }

    /// Whether the exchange carried a JSON-RPC request: known from the paired
    /// body, or — with no request side recorded — from a response following
    /// the status.
    pub(in crate::checks) fn carried_request(&self) -> bool {
        self.carried.map_or_else(
            || self.request.is_none() && self.framed_response,
            |payload| {
                payload.get("method").is_some() && payload.get("id").is_some_and(|id| !id.is_null())
            },
        )
    }

    /// The response's media type, lowercased and without parameters.
    pub(in crate::checks) fn media_type(&self) -> Option<String> {
        self.headers.get("content-type").map(|content_type| {
            content_type
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        })
    }
}

/// The order-derived HTTP framing of a trace.
#[derive(Debug, Default)]
pub(in crate::checks) struct Framing<'a> {
    posts: Vec<Post<'a>>,
    exchanges: Vec<Exchange<'a>>,
    /// Response message `seq` → the status `(seq, code)` it rode.
    statuses: BTreeMap<u64, (u64, u16)>,
    /// Null-`id` response message `seq` → the `seq` of the request message it
    /// answers, where the framing says.
    answers: BTreeMap<u64, u64>,
}

impl<'a> Framing<'a> {
    /// Derives the framing of `context`'s Streamable HTTP events in one pass.
    pub(in crate::checks) fn new(context: &'a TraceContext<'_>) -> Self {
        let mut builder = Builder::default();
        for event in context.events() {
            if event.transport == TransportKind::StreamableHttp {
                builder.step(event);
            }
        }
        builder.framing
    }

    /// The POSTs whose headers and body the trace pairs unambiguously, in order.
    pub(in crate::checks::stateless) fn posts(&self) -> &[Post<'a>] {
        &self.posts
    }

    /// The exchanges whose response status the trace attributes, in the order
    /// their statuses were recorded.
    pub(in crate::checks) fn exchanges(&self) -> &[Exchange<'a>] {
        &self.exchanges
    }

    /// The status the response message at `seq` rode, when the trace says.
    pub(in crate::checks::stateless) fn status_for(&self, seq: u64) -> Option<(u64, u16)> {
        self.statuses.get(&seq).copied()
    }

    /// The null-`id` responses the framing ties to a request, as
    /// `(response seq, request message seq)`, in trace order.
    pub(in crate::checks::stateless) fn unidentified_answers(
        &self,
    ) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.answers
            .iter()
            .map(|(response, request)| (*response, *request))
    }
}

/// The one-pass state behind [`Framing::new`].
#[derive(Default)]
struct Builder<'a> {
    framing: Framing<'a>,
    /// Client exchanges awaiting a status, oldest first.
    awaiting: Vec<Slot<'a>>,
    /// Body-bearing slots whose body has not been seen.
    open_bodies: usize,
    /// Whether two body-bearing slots overlapped since none was last open.
    overlapped: bool,
    /// Request id text → (request message seq, the status attributed to it).
    requests: BTreeMap<String, (u64, Option<(u64, u16)>)>,
    /// The status just recorded, and whose it is, until a server message
    /// follows it.
    fresh_status: Option<((u64, u16), Owner)>,
    /// The exchange the status just recorded was attributed to, until a server
    /// message follows it.
    fresh_exchange: Option<usize>,
}

/// Whose response a status began.
#[derive(Debug, Clone, Copy)]
enum Owner {
    /// The request message at this `seq`, unambiguously.
    Request(u64),
    /// No recorded client exchange was awaiting one: a recording that carries
    /// response framing but not request framing. The message immediately after
    /// it is the body it framed — the only reading such a trace supports.
    Unaccounted,
    /// An exchange whose identity the order cannot settle.
    Unknown,
}

/// A message's `id` as canonical text, when it has a non-null one.
fn id_text(payload: &Value) -> Option<String> {
    payload
        .get("id")
        .filter(|id| !id.is_null())
        .map(ToString::to_string)
}

impl<'a> Builder<'a> {
    fn step(&mut self, event: &'a TraceEvent) {
        match (&event.body, event.direction) {
            (
                EventBody::Http {
                    method, headers, ..
                },
                Direction::ClientToServer,
            ) => {
                let has_body = !matches!(method.as_deref(), Some("GET" | "DELETE"));
                if has_body {
                    self.open_bodies += 1;
                    self.overlapped |= self.open_bodies > 1;
                }
                self.awaiting.push(Slot {
                    seq: event.seq,
                    method: method.as_deref(),
                    headers,
                    has_body,
                    request: None,
                    carried: None,
                    tainted: false,
                });
            }
            (EventBody::Message { payload }, Direction::ClientToServer) => {
                self.client_message(event.seq, payload);
            }
            (
                EventBody::Http {
                    status: Some(status),
                    headers,
                    ..
                },
                Direction::ServerToClient,
            ) => self.response_began(Some((event.seq, *status, headers))),
            // The capture records a proxy-side upstream failure in place of the
            // status the exchange never got: it ends one exchange, and says
            // nothing about which.
            (
                EventBody::Lifecycle {
                    event: LifecycleEvent::TransportAbort,
                },
                Direction::ServerToClient,
            ) => self.response_began(None),
            (EventBody::Message { payload }, Direction::ServerToClient) => {
                self.server_message(event.seq, payload);
            }
            _ => {}
        }
    }

    fn client_message(&mut self, seq: u64, payload: &'a Value) {
        let id = id_text(payload);
        if let Some(id) = &id
            && payload.get("method").is_some()
        {
            // Registered even when unpaired, so a later request reusing the id
            // can never inherit an earlier one's status.
            self.requests.insert(id.clone(), (seq, None));
        }
        if self.open_bodies == 0 {
            return; // No POST awaits a body: an unframed message.
        }
        let certain = self.open_bodies == 1 && !self.overlapped;
        self.open_bodies -= 1;
        if self.open_bodies == 0 {
            self.overlapped = false;
        }
        if !certain {
            return;
        }
        let Some(slot) = self
            .awaiting
            .iter_mut()
            .rev()
            .find(|slot| slot.has_body && slot.request.is_none())
        else {
            return;
        };
        slot.request = Some((seq, id));
        slot.carried = Some(payload);
        self.framing.posts.push(Post {
            seq: slot.seq,
            message_seq: seq,
            headers: slot.headers,
            payload,
        });
    }

    /// A response status (or an abort standing in for one) ends one exchange.
    fn response_began(&mut self, status: Option<(u64, u16, &'a BTreeMap<String, String>)>) {
        self.fresh_status = None;
        self.fresh_exchange = None;
        if self.awaiting.is_empty() {
            if let Some((seq, code, headers)) = status {
                self.fresh_status = Some(((seq, code), Owner::Unaccounted));
                self.push_exchange(None, None, seq, code, headers);
            }
            return;
        }
        let ambiguous = self.awaiting.len() > 1;
        let slot = self.awaiting.remove(0);
        if ambiguous {
            // Which exchange this status ended is unknown: none of the ones
            // still waiting can be told apart from the one that was answered.
            for waiting in &mut self.awaiting {
                waiting.tainted = true;
            }
        }
        let Some((status_seq, code, headers)) = status else {
            return;
        };
        let status = (status_seq, code);
        if !ambiguous && !slot.tainted {
            self.push_exchange(
                Some((slot.seq, slot.method, slot.headers)),
                slot.carried,
                status_seq,
                code,
                headers,
            );
        }
        let attributed = (!ambiguous && !slot.tainted)
            .then_some(slot.request)
            .flatten();
        let owner = attributed
            .as_ref()
            .map_or(Owner::Unknown, |(seq, _)| Owner::Request(*seq));
        self.fresh_status = Some((status, owner));
        if let Some((request_seq, Some(id))) = attributed
            && let Some(entry) = self.requests.get_mut(&id)
            && entry.0 == request_seq
        {
            entry.1 = Some(status);
        }
    }

    fn push_exchange(
        &mut self,
        request: Option<(u64, Option<&'a str>, &'a BTreeMap<String, String>)>,
        carried: Option<&'a Value>,
        status_seq: u64,
        status: u16,
        headers: &'a BTreeMap<String, String>,
    ) {
        self.fresh_exchange = Some(self.framing.exchanges.len());
        self.framing.exchanges.push(Exchange {
            request,
            carried,
            status_seq,
            status,
            headers,
            framed_response: false,
        });
    }

    fn server_message(&mut self, seq: u64, payload: &Value) {
        let fresh = self.fresh_status.take();
        let fresh_exchange = self.fresh_exchange.take();
        let is_response = payload.get("method").is_none()
            && (payload.get("result").is_some() || payload.get("error").is_some());
        if !is_response {
            return;
        }
        if let Some(exchange) =
            fresh_exchange.and_then(|index| self.framing.exchanges.get_mut(index))
        {
            exchange.framed_response = true;
        }
        match (id_text(payload), fresh) {
            (Some(id), fresh) => {
                if let Some((_, Some(status))) = self.requests.get(&id) {
                    self.framing.statuses.insert(seq, *status);
                } else if let Some((status, Owner::Unaccounted)) = fresh {
                    self.framing.statuses.insert(seq, status);
                }
            }
            (None, Some((status, Owner::Request(request_seq)))) => {
                self.framing.statuses.insert(seq, status);
                self.framing.answers.insert(seq, request_seq);
            }
            (None, _) => {}
        }
    }
}
