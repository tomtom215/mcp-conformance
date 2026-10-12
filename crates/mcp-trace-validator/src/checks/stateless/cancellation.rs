// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! basic/patterns/cancellation: the one cancellation a server may send.
//!
//! `2026-07-28` removed server-initiated requests, so a server has nothing of
//! its own to cancel and nothing of the client's to abort by notification: the
//! page leaves it exactly one `notifications/cancelled`, the one that ends a
//! `subscriptions/listen` stream. The client's cancellations are judged on the
//! transport pages (TRAN-123, TRAN-124); most of this page's other clauses bind
//! timers and internal handling and carry exclusions in the registry.

use std::collections::BTreeMap;

use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The cancellation notification.
const CANCELLED: &str = "notifications/cancelled";

/// The only request a server-sent cancellation may name.
const LISTEN: &str = "subscriptions/listen";

/// `CANC-003` and `CANC-011`: a server sends `notifications/cancelled` only to
/// end a `subscriptions/listen` stream.
///
/// The cancelled id is resolved against the latest client request the trace
/// shows carrying it — ids may be reused once answered (BASE-045), so the
/// latest is the one a cancellation can still mean. An id the recording never
/// saw a request for is not reported: a session captured mid-way may have opened
/// that subscription before the capture began. A cancellation naming no request
/// at all is reported, since it cannot be the teardown the page permits.
pub(in crate::checks) fn server_cancels_only_subscriptions(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Request id (canonical text) → the method of the latest client request with it.
    let mut methods: BTreeMap<String, &str> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let method = payload.get("method").and_then(Value::as_str);
        let id = payload.get("id").filter(|id| !id.is_null());
        if event.direction == Direction::ClientToServer {
            if let (Some(method), Some(id)) = (method, id) {
                methods.insert(id.to_string(), method);
            }
            continue;
        }
        if method != Some(CANCELLED) || id.is_some() {
            continue;
        }
        let Some(request_id) = payload
            .get("params")
            .and_then(|params| params.get("requestId"))
            .filter(|id| !id.is_null())
        else {
            sink.examined();
            sink.push(
                Some(event.seq),
                format!(
                    "server sent `{CANCELLED}` with no `params.requestId`; a server may only \
                     cancel a `{LISTEN}` request, by naming it"
                ),
            );
            continue;
        };
        let Some(cancelled) = methods.get(&request_id.to_string()) else {
            continue;
        };
        // The subject is a server cancellation of a request the trace shows.
        sink.examined();
        if *cancelled != LISTEN {
            sink.push(
                Some(event.seq),
                format!(
                    "server sent `{CANCELLED}` for request {request_id}, a `{cancelled}`; at \
                     2026-07-28 a server cancels nothing but a `{LISTEN}` request, to end \
                     that subscription"
                ),
            );
        }
    }
}
