// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The session manager behind the HTTP service: rmcp's
//! [`LocalSessionManager`], plus the two things `test_reconnection` needs
//! that rmcp keeps to itself.
//!
//! - **Which stream a request arrived on.** rmcp numbers each POST's SSE
//!   stream inside the session worker and never shows a handler the number.
//!   [`ResumableSessions::create_stream`] therefore performs the same three
//!   calls rmcp's own does — through `LocalSessionHandle`'s public API — and
//!   attaches a [`PostStream`] to the request in between.
//! - **When a client resumes a stream.** [`ResumableSessions::resume`] tells
//!   the waiting tool call, once rmcp has re-attached the stream.
//!
//! Everything else delegates unchanged.

use std::sync::Arc;

use rmcp::model::{ClientJsonRpcMessage, GetExtensions as _, JsonRpcMessage, ServerJsonRpcMessage};
use rmcp::transport::WorkerTransport;
use rmcp::transport::common::server_side_http::ServerSseMessage;
use rmcp::transport::streamable_http_server::session::local::{
    LocalSessionManager, LocalSessionManagerError, LocalSessionWorker,
};
use rmcp::transport::streamable_http_server::{RestoreOutcome, SessionId, SessionManager};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;

use super::{PostStream, Resumptions};

/// [`LocalSessionManager`] with per-request stream identity and resume
/// announcements; see the module documentation.
#[derive(Debug, Default)]
pub struct ResumableSessions {
    inner: LocalSessionManager,
    resumptions: Arc<Resumptions>,
}

/// The POST stream an event id belongs to. rmcp writes request-stream event
/// ids as `<index>/<http request id>` and standalone-stream ids as a bare
/// `<index>`; anything else is not a request stream's id.
fn request_stream_of(last_event_id: &str) -> Option<u64> {
    last_event_id.split_once('/')?.1.parse().ok()
}

impl SessionManager for ResumableSessions {
    type Error = LocalSessionManagerError;
    type Transport = WorkerTransport<LocalSessionWorker>;

    async fn create_session(&self) -> Result<(SessionId, Self::Transport), Self::Error> {
        self.inner.create_session().await
    }

    async fn initialize_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, Self::Error> {
        self.inner.initialize_session(id, message).await
    }

    async fn has_session(&self, id: &SessionId) -> Result<bool, Self::Error> {
        self.inner.has_session(id).await
    }

    async fn close_session(&self, id: &SessionId) -> Result<(), Self::Error> {
        self.inner.close_session(id).await
    }

    async fn create_stream(
        &self,
        id: &SessionId,
        mut message: ClientJsonRpcMessage,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        // Cloned out so the map's lock is released before the worker is
        // awaited, as rmcp's own lookup does.
        let handle = self.inner.sessions.read().await.get(id).cloned();
        let handle = handle.ok_or_else(|| LocalSessionManagerError::SessionNotFound(id.clone()))?;
        let receiver = handle.establish_request_wise_channel().await?;
        let http_request_id = receiver.http_request_id;
        if let (Some(http_request_id), JsonRpcMessage::Request(request)) =
            (http_request_id, &mut message)
        {
            request.request.extensions_mut().insert(PostStream {
                handle: handle.clone(),
                http_request_id,
                resumptions: Arc::clone(&self.resumptions),
            });
        }
        handle.push_message(message, http_request_id).await?;
        Ok(ReceiverStream::new(receiver.inner))
    }

    async fn accept_message(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<(), Self::Error> {
        self.inner.accept_message(id, message).await
    }

    async fn create_standalone_stream(
        &self,
        id: &SessionId,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        self.inner.create_standalone_stream(id).await
    }

    async fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        let resumed = request_stream_of(&last_event_id);
        let stream = self.inner.resume(id, last_event_id).await?;
        // Announced only after rmcp has re-attached the stream: the tool's
        // result must find the new stream in place, not race it.
        if let Some(http_request_id) = resumed {
            self.resumptions.announce(&(id.clone(), http_request_id));
        }
        Ok(stream)
    }

    async fn restore_session(
        &self,
        id: SessionId,
    ) -> Result<RestoreOutcome<Self::Transport>, Self::Error> {
        self.inner.restore_session(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::request_stream_of;

    #[test]
    fn only_request_stream_event_ids_name_a_post_stream() {
        assert_eq!(request_stream_of("1/7"), Some(7));
        assert_eq!(request_stream_of("0/0"), Some(0));
        // The standalone stream's ids carry no request number.
        assert_eq!(request_stream_of("4"), None);
        assert_eq!(request_stream_of("1/x"), None);
        assert_eq!(request_stream_of(""), None);
    }
}
