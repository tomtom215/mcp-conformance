// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` basic utilities: `basic/utilities/ping`
//! (`PING-*`), `basic/utilities/progress` (`PROG-009` onwards) and
//! `basic/utilities/cancellation` (`CANC-018` onwards).
//!
//! The three pages were out of the registry's scope until 2026-10-24 as an
//! "unclaimed surface", so a server sending progress for a token nobody issued,
//! or a cancellation naming a request nobody sent, judged clean at this
//! revision while the same traffic failed at `2026-07-28`.
//!
//! **Why these are not the `2026-07-28` checks.** `stateless::progress` and
//! `stateless::cancellation` judge the same utilities, but at a revision where
//! only the client sends requests, so each reads one direction: the client's
//! tokens, the server's notifications. At `2025-11-25` both parties send
//! requests — a server's `sampling/createMessage` or `roots/list` may opt into
//! progress and may be cancelled by the server that sent it — so every rule
//! here binds whichever party is the *sender* of the request, and every check
//! reads both directions. The sentences that are identical at both revisions
//! are entered under new ids ([`STABILITY.md`]: an id names one clause at one
//! revision, forever), judged by checks of their own.
//!
//! **What a recording can witness here that it cannot at `2026-07-28`.** A
//! `2025-11-25` session begins with `initialize` (LIFE-001), so a recording that
//! holds the handshake holds every request the session issued after it. That is
//! the witness two clauses need — a progress token no request provided, a
//! cancellation naming a request never issued — and the stateless revision
//! has no equivalent, which is why CANC-010 carries an exclusion there and
//! CANC-018 carries a check here. Without the handshake both checks abstain.
//!
//! [`STABILITY.md`]: https://github.com/tomtom215/mcp-conformance/blob/main/STABILITY.md

use std::collections::BTreeMap;

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

use crate::context::TraceContext;

pub(super) mod cancellation;
pub(super) mod ping;
pub(super) mod progress;

#[cfg(test)]
mod testkit;

/// One of the two parties, as the sender of a message: the key every rule here
/// scopes a token or an id by, since each party issues its own requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Party {
    Client,
    Server,
}

impl Party {
    /// The party that sends messages in `direction`.
    const fn of(direction: Direction) -> Self {
        match direction {
            Direction::ClientToServer => Self::Client,
            Direction::ServerToClient => Self::Server,
        }
    }

    /// The party a message from `direction` is addressed to.
    const fn addressed_by(direction: Direction) -> Self {
        Self::of(direction).other()
    }

    /// The other party.
    const fn other(self) -> Self {
        match self {
            Self::Client => Self::Server,
            Self::Server => Self::Client,
        }
    }

    /// How a finding names the party.
    const fn name(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }
}

/// A request's `id` as canonical text, when `payload` is a request: a `method`
/// and a non-null `id`. Canonical text, so the string `"1"` and the integer `1`
/// stay the different ids they are.
fn request_id(payload: &Value) -> Option<String> {
    payload.get("method")?.as_str()?;
    payload
        .get("id")
        .filter(|id| !id.is_null())
        .map(to_canonical_string)
}

/// A response's `id` as canonical text, when `payload` is one: no `method` and
/// a non-null `id`.
fn response_id(payload: &Value) -> Option<String> {
    if payload.get("method").is_some() {
        return None;
    }
    payload
        .get("id")
        .filter(|id| !id.is_null())
        .map(to_canonical_string)
}

/// Whether `event` is a notification of `method`: that method, and no id or a
/// null one.
fn is_notification(event: &TraceEvent, method: &str) -> bool {
    event.message_payload().is_some_and(|payload| {
        payload.get("method").and_then(Value::as_str) == Some(method)
            && payload.get("id").is_none_or(Value::is_null)
    })
}

/// A notification's `params`, when `event` is a notification of `method` that
/// carries any.
fn notification_params<'e>(event: &'e TraceEvent, method: &str) -> Option<&'e Value> {
    if !is_notification(event, method) {
        return None;
    }
    event.message_payload()?.get("params")
}

/// The `seq` of the client's first `initialize` request, if the recording holds
/// one: the witness that the session began inside the recording, so every
/// request it issued afterwards is in it.
fn session_start(context: &TraceContext<'_>) -> Option<u64> {
    context.messages().find_map(|(event, _, _)| {
        if event.direction != Direction::ClientToServer {
            return None;
        }
        let payload = event.message_payload()?;
        request_id(payload)?;
        (payload.get("method")?.as_str()? == "initialize").then_some(event.seq)
    })
}

/// Every answer in either direction, keyed by the answer's `seq`: answer → the
/// `seq` of the request it answers. Keyed by the answer so a walk in trace order
/// learns a request finished at the moment it reaches the answer.
fn answers(context: &TraceContext<'_>) -> BTreeMap<u64, u64> {
    context
        .exchanges()
        .map(|exchange| (exchange.response.seq, exchange.request.seq))
        .collect()
}
