// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The HTTP client that authorizes when the MCP server says it must.
//!
//! [`Authorizing`] wraps any rmcp `StreamableHttpClient` and is one itself, so
//! rmcp's own streamable-HTTP transport runs on top of it unchanged. Requests
//! go out with whatever token is held — none, to begin with. A `401` runs the
//! [`Flow`] from its challenge; a `403 insufficient_scope` naming a scope runs
//! the escalation; then the same request is sent again. Nothing is authorized
//! ahead of a challenge, because the suite's servers (like real ones) let
//! `initialize` through and challenge a later request.
//!
//! The loop ends: every pass that sends again either spent one of the
//! session's authorization requests or found a token somebody else just
//! obtained, and the budget is finite.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use futures::stream::BoxStream;
use http::{HeaderName, HeaderValue};
use rmcp::model::ClientJsonRpcMessage;
use rmcp::transport::auth::AuthError;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use sse_stream::{Error as SseError, Sse};

use super::flow::{Flow, FlowError};

/// A `StreamableHttpClient` that obtains and escalates its own authorization.
#[derive(Debug)]
pub struct Authorizing<C> {
    inner: C,
    flow: Arc<Flow>,
}

impl<C: Clone> Clone for Authorizing<C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            flow: Arc::clone(&self.flow),
        }
    }
}

impl<C> Authorizing<C> {
    /// `inner`, authorized through `flow`.
    pub const fn new(inner: C, flow: Arc<Flow>) -> Self {
        Self { inner, flow }
    }
}

/// What a refused request leads to.
enum Next<E: std::error::Error + Send + Sync + 'static> {
    /// A token is in place: send the request again.
    Resend,
    /// Nothing more to try: this is the request's outcome.
    Fail(StreamableHttpError<E>),
}

/// The outcome of an authorization attempt, where `refusal` is the error the
/// server's refusal arrived as.
///
/// A spent budget, or a client with no user to ask, leaves the refusal itself
/// as the outcome: the server said no and the host has stopped arguing.
fn next<E>(attempt: Result<(), FlowError>, refusal: StreamableHttpError<E>) -> Next<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match attempt {
        Ok(()) => Next::Resend,
        Err(FlowError::BudgetSpent | FlowError::NoUser) => Next::Fail(refusal),
        Err(FlowError::Auth(error)) => Next::Fail(StreamableHttpError::Auth(error)),
        Err(FlowError::Consent(error)) => Next::Fail(StreamableHttpError::Auth(
            AuthError::AuthorizationFailed(error.to_string()),
        )),
    }
}

impl<C: StreamableHttpClient + Sync> Authorizing<C> {
    /// Runs `call` with the held token, authorizing and resending for as long
    /// as the server's refusals can be answered.
    async fn reacting<T, F, Fut>(&self, call: F) -> Result<T, StreamableHttpError<C::Error>>
    where
        F: Fn(Option<String>) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, StreamableHttpError<C::Error>>> + Send,
        T: Send,
    {
        loop {
            let token = self.flow.token().await?;
            let refusal = match call(token.clone()).await {
                Err(
                    refusal @ (StreamableHttpError::AuthRequired(_)
                    | StreamableHttpError::InsufficientScope(_)),
                ) => refusal,
                outcome => return outcome,
            };
            let attempt = match &refusal {
                StreamableHttpError::AuthRequired(challenge) => {
                    let header = &challenge.www_authenticate_header;
                    self.flow.authorize(header, token.as_deref()).await
                }
                StreamableHttpError::InsufficientScope(challenge) => {
                    match challenge.get_required_scope() {
                        Some(scope) => self.flow.step_up(scope, token.as_deref()).await,
                        // No scope named: nothing to ask the authorization
                        // server for that it has not already refused.
                        None => return Err(refusal),
                    }
                }
                _ => return Err(refusal),
            };
            match next(attempt, refusal) {
                Next::Resend => {}
                Next::Fail(error) => return Err(error),
            }
        }
    }
}

impl<C: StreamableHttpClient + Sync> StreamableHttpClient for Authorizing<C> {
    type Error = C::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.reacting(|token| {
            self.inner.post_message(
                uri.clone(),
                message.clone(),
                session_id.clone(),
                token,
                custom_headers.clone(),
            )
        })
        .await
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.reacting(|token| {
            self.inner.post_message_with_max_sse_event_size(
                uri.clone(),
                message.clone(),
                session_id.clone(),
                token,
                custom_headers.clone(),
                max_sse_event_size,
            )
        })
        .await
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        self.reacting(|token| {
            self.inner.delete_session(
                uri.clone(),
                session_id.clone(),
                token,
                custom_headers.clone(),
            )
        })
        .await
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        self.reacting(|token| {
            self.inner.get_stream(
                uri.clone(),
                session_id.clone(),
                last_event_id.clone(),
                token,
                custom_headers.clone(),
            )
        })
        .await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        self.reacting(|token| {
            self.inner.get_stream_with_max_sse_event_size(
                uri.clone(),
                session_id.clone(),
                last_event_id.clone(),
                token,
                custom_headers.clone(),
                max_sse_event_size,
            )
        })
        .await
    }
}
