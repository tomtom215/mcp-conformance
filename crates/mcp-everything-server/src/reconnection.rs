// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `test_reconnection`: the server-initiated SSE disconnect of SEP-1699.
//!
//! The official suite's `server-sse-polling` scenario calls this tool and
//! expects the server to end the POST's SSE stream *before* answering — after
//! an event carrying an id and a `retry` interval — and to deliver the result
//! on the stream the client opens next, a GET presenting `Last-Event-ID`.
//!
//! rmcp can close a request's stream ([`LocalSessionHandle::close_sse_stream`])
//! but hands a tool handler neither the session handle nor the transport's
//! number for its own stream. [`sessions::ResumableSessions`] supplies both:
//! it is the session manager the HTTP service is built on, and it attaches a
//! [`PostStream`] to every request it routes. Where no such attachment exists
//! — stdio, and `2026-07-28`'s session-less HTTP — there is no stream to
//! close, and the tool simply returns its result.
//!
//! The wait for the client's return is an event, not a delay. rmcp drops a
//! request's stream state the moment its response is sent, so a result that
//! beat the reconnecting GET would be lost; the tool therefore holds its
//! result until the session manager has seen that GET (bounded by
//! [`RESUME_WAIT`], for a client that never comes back).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::SessionId;
use rmcp::transport::streamable_http_server::session::local::LocalSessionHandle;
use rmcp::{ErrorData, RoleServer, tool, tool_router};
use tokio::sync::Notify;

use crate::server::EverythingServer;

pub mod sessions;

/// Text of the tool's result; the scenario only requires *a* result, and this
/// wording matches the other SDKs' fixture.
pub const RECONNECTION_RESULT: &str = "Reconnection test completed";

/// The `retry` interval sent with the event that precedes the disconnect: how
/// long the client is asked to wait before polling again.
pub const RECONNECT_RETRY: Duration = Duration::from_millis(500);

/// Upper bound on how long the tool holds its result for the resuming GET.
///
/// Generous next to [`RECONNECT_RETRY`]; reached only by a client that does
/// not resume, whose result has nowhere to go either way.
pub const RESUME_WAIT: Duration = Duration::from_secs(5);

/// The POST SSE stream a request arrived on, attached to that request's
/// extensions by [`sessions::ResumableSessions`].
#[derive(Debug, Clone)]
pub struct PostStream {
    handle: LocalSessionHandle,
    http_request_id: u64,
    resumptions: Arc<Resumptions>,
}

impl PostStream {
    /// Closes the stream — after one more event carrying [`RECONNECT_RETRY`]
    /// — and waits until the client has resumed it, or [`RESUME_WAIT`] has
    /// passed. Returns whether the client came back.
    ///
    /// A stream that cannot be closed (the session is gone) is not waited on.
    pub async fn close_until_resumed(&self) -> bool {
        let key = (self.handle.id().clone(), self.http_request_id);
        let awaited = self.resumptions.expect(key);
        if self
            .handle
            .close_sse_stream(self.http_request_id, Some(RECONNECT_RETRY))
            .await
            .is_err()
        {
            return false;
        }
        tokio::time::timeout(RESUME_WAIT, awaited.signal.notified())
            .await
            .is_ok()
    }
}

/// A stream within the transport: the session, and rmcp's per-session number
/// for the POST request that opened it.
type StreamKey = (SessionId, u64);

/// The streams whose resumption a tool call is currently waiting for.
#[derive(Debug, Default)]
struct Resumptions(Mutex<HashMap<StreamKey, Arc<Notify>>>);

impl Resumptions {
    /// Registers interest in `key` until the returned guard is dropped.
    fn expect(self: &Arc<Self>, key: StreamKey) -> Awaited {
        let signal = Arc::new(Notify::new());
        self.streams().insert(key.clone(), Arc::clone(&signal));
        Awaited {
            resumptions: Arc::clone(self),
            key,
            signal,
        }
    }

    /// Wakes the call waiting on `key`, if there is one. `notify_one` stores
    /// its permit, so a resume that lands before the waiter polls is not lost.
    fn announce(&self, key: &StreamKey) {
        if let Some(signal) = self.streams().get(key) {
            signal.notify_one();
        }
    }

    /// The map, recovered from a poisoned lock: every critical section is a
    /// single map operation, so there is no half-applied state to distrust.
    fn streams(&self) -> std::sync::MutexGuard<'_, HashMap<StreamKey, Arc<Notify>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One registered wait; dropping it (completion, timeout or a cancelled call)
/// removes the registration, so the map holds only calls in flight.
struct Awaited {
    resumptions: Arc<Resumptions>,
    key: StreamKey,
    signal: Arc<Notify>,
}

impl Drop for Awaited {
    fn drop(&mut self) {
        self.resumptions.streams().remove(&self.key);
    }
}

#[tool_router(router = tool_router_reconnection, vis = "pub(crate)")]
impl EverythingServer {
    /// `server-sse-polling`: closes the SSE stream this call arrived on, waits
    /// for the client to resume it with `Last-Event-ID`, and answers there.
    /// On a transport with no such stream it answers directly.
    ///
    /// # Errors
    ///
    /// Never fails: a stream that cannot be closed, or a client that does not
    /// return, still gets the result sent.
    #[tool(
        description = "Closes the SSE stream mid-call and delivers the result after the client reconnects (SEP-1699), for conformance testing"
    )]
    pub async fn test_reconnection(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(stream) = context.extensions.get::<PostStream>() {
            stream.close_until_resumed().await;
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(
            RECONNECTION_RESULT,
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(request: u64) -> StreamKey {
        (SessionId::from("session"), request)
    }

    #[tokio::test]
    async fn an_announcement_reaches_the_wait_registered_for_that_stream() {
        let resumptions = Arc::new(Resumptions::default());
        let awaited = resumptions.expect(key(3));
        // Announced before the waiter polls: the stored permit must carry it.
        resumptions.announce(&key(3));
        let woken = tokio::select! {
            biased;
            () = awaited.signal.notified() => true,
            () = std::future::ready(()) => false,
        };
        assert!(woken);
    }

    #[tokio::test]
    async fn another_streams_announcement_does_not_wake_the_wait() {
        let resumptions = Arc::new(Resumptions::default());
        let awaited = resumptions.expect(key(3));
        resumptions.announce(&key(4));
        // One biased poll: a stored permit would complete the first arm.
        let woken = tokio::select! {
            biased;
            () = awaited.signal.notified() => true,
            () = std::future::ready(()) => false,
        };
        assert!(!woken);
    }

    #[test]
    fn a_finished_wait_leaves_no_registration_behind() {
        let resumptions = Arc::new(Resumptions::default());
        let awaited = resumptions.expect(key(3));
        assert_eq!(resumptions.streams().len(), 1);
        drop(awaited);
        assert!(resumptions.streams().is_empty());
    }
}
