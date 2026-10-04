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
    /// The request's headers.
    headers: &'a BTreeMap<String, String>,
    /// Whether this exchange carries a body to pair (not a `GET` or `DELETE`).
    has_body: bool,
    /// The paired request message: its `seq` and `id` text, once paired.
    request: Option<(u64, Option<String>)>,
    /// Whether a status arrived while this slot and another both awaited one,
    /// so that a later status cannot be told apart as this slot's.
    tainted: bool,
}

/// The order-derived HTTP framing of a trace.
#[derive(Debug, Default)]
pub(in crate::checks::stateless) struct Framing<'a> {
    posts: Vec<Post<'a>>,
    /// Response message `seq` → the status `(seq, code)` it rode.
    statuses: BTreeMap<u64, (u64, u16)>,
    /// Null-`id` response message `seq` → the `seq` of the request message it
    /// answers, where the framing says.
    answers: BTreeMap<u64, u64>,
}

impl<'a> Framing<'a> {
    /// Derives the framing of `context`'s Streamable HTTP events in one pass.
    pub(in crate::checks::stateless) fn new(context: &'a TraceContext<'_>) -> Self {
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
                    headers,
                    has_body,
                    request: None,
                    tainted: false,
                });
            }
            (EventBody::Message { payload }, Direction::ClientToServer) => {
                self.client_message(event.seq, payload);
            }
            (
                EventBody::Http {
                    status: Some(status),
                    ..
                },
                Direction::ServerToClient,
            ) => self.response_began(Some((event.seq, *status))),
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
        self.framing.posts.push(Post {
            seq: slot.seq,
            message_seq: seq,
            headers: slot.headers,
            payload,
        });
    }

    /// A response status (or an abort standing in for one) ends one exchange.
    fn response_began(&mut self, status: Option<(u64, u16)>) {
        self.fresh_status = None;
        if self.awaiting.is_empty() {
            self.fresh_status = status.map(|status| (status, Owner::Unaccounted));
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
        let Some(status) = status else { return };
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

    fn server_message(&mut self, seq: u64, payload: &Value) {
        let fresh = self.fresh_status.take();
        let is_response = payload.get("method").is_none()
            && (payload.get("result").is_some() || payload.get("error").is_some());
        if !is_response {
            return;
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
