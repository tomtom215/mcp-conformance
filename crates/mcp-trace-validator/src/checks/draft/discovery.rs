// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `2026-07-28` discovery checks (`server/discover`).
//!
//! Two of the page's four clauses are wire-observable and judged here. The
//! other two bind a server's self-identification policy and a client's private
//! use of `serverInfo`, and carry documented exclusions in the registry.
//!
//! Both checks read the classification the context already made
//! ([`MessageKind`]) rather than re-deriving message shape from the payload:
//! the only thing they need beyond it is the request's `_meta`, which is a
//! payload lookup by design.

use std::collections::BTreeMap;

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The discovery probe every `2026-07-28` server must answer.
const DISCOVER: &str = "server/discover";

/// The removed handshake. A client that sends it is, by that act, a client that
/// still speaks the legacy era — the method exists in no other.
const INITIALIZE: &str = "initialize";

/// The `_meta` field by which a request declares the era it speaks.
const PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// JSON-RPC `Method not found`.
const METHOD_NOT_FOUND: i64 = -32601;

/// `DISC-001`: servers implement `server/discover`.
///
/// Falsified by the one answer that proves absence — `-32601` (`Method not
/// found`) to a `server/discover` request. Other errors do not prove it:
/// `-32022` (unsupported protocol version) and `-32602` are answers *from* an
/// implementation. A session that never probes says nothing either way, so this
/// abstains rather than reporting the method missing; a MUST that no recorded
/// message bears on is not a MUST the trace can fail.
///
/// A legacy server's session, judged at this revision, does report here. That is
/// the correct reading and not a false positive: the clause binds servers at
/// `2026-07-28`, and a server answering `-32601` is not one — the *client's*
/// conduct in that same exchange is DISC-002's business, separately.
pub(in crate::checks) fn implemented(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let probes: BTreeMap<String, u64> = context
        .messages()
        .filter_map(|(event, kind, _)| match kind {
            MessageKind::Request { method, id }
                if *method == DISCOVER && event.direction == Direction::ClientToServer =>
            {
                Some((id.to_string(), event.seq))
            }
            _ => None,
        })
        .collect();
    if probes.is_empty() {
        return;
    }
    for (event, kind, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        let MessageKind::Error {
            id: Some(id),
            error,
        } = kind
        else {
            continue;
        };
        if !probes.contains_key(&id.to_string()) {
            continue;
        }
        if error.get("code").and_then(Value::as_i64) == Some(METHOD_NOT_FOUND) {
            sink.push(
                Some(event.seq),
                "`server/discover` was answered with -32601 (method not found); \
                 2026-07-28 servers must implement it"
                    .to_owned(),
            );
        }
    }
}

/// `DISC-002`: a client that speaks both eras probes with `server/discover` first.
///
/// The clause's antecedent — "supports both modern \[…\] and legacy \[…\] servers"
/// — is a property of the client, not of the wire, so this fires only when the
/// session witnesses *both* halves of it: an `initialize` request (legacy; the
/// method exists in no other era) and a request carrying `PROTOCOL_VERSION` in
/// its `_meta` (modern). A legacy-only client shows the first and never the
/// second, and is not judged here, because it does not match the antecedent —
/// getting that wrong would report every legacy session as a client defect.
///
/// "First" is the clause's own word and is read literally: the probe must be the
/// client's first *request*. Notifications do not count, matching
/// `basic/transports/stdio#backward-compatibility`, which puts the probe
/// "before sending any other request".
pub(in crate::checks) fn dual_era_probe_first(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut first: Option<(u64, &str)> = None;
    let mut legacy = false;
    let mut modern = false;
    for (event, kind, _) in context.messages() {
        if event.direction != Direction::ClientToServer {
            continue;
        }
        let MessageKind::Request { method, .. } = kind else {
            continue;
        };
        if first.is_none() {
            first = Some((event.seq, method));
        }
        legacy |= *method == INITIALIZE;
        modern |= event
            .message_payload()
            .and_then(|payload| payload.get("params")?.get("_meta")?.get(PROTOCOL_VERSION))
            .is_some();
    }
    let Some((seq, method)) = first else {
        return;
    };
    if legacy && modern && method != DISCOVER {
        sink.push(
            Some(seq),
            format!(
                "the client's first request is `{method}`, but this session shows both \
                 eras (an `initialize` request and a modern `_meta` protocol version), \
                 so it should have probed with `server/discover` first"
            ),
        );
    }
}
