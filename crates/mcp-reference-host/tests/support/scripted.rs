// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A scripted `StreamableHttpClient`: it answers from a queue, then with a
//! standing reply, and logs every request it was given — which method, with
//! which token and which arguments.

#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::expect_used)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use futures::StreamExt as _;
use futures::stream::BoxStream;
use http::{HeaderName, HeaderValue};
use rmcp::model::ClientJsonRpcMessage;
use rmcp::transport::streamable_http_client::{
    AuthRequiredError, InsufficientScopeError, StreamableHttpClient, StreamableHttpError,
    StreamableHttpPostResponse,
};
use sse_stream::{Error as SseError, Sse};

use crate::support::Site;

pub type Failure = StreamableHttpError<std::io::Error>;

/// More requests than any test here sends: one sign-in, two escalations and
/// the four sends around them, three times over.
const RUNAWAY: usize = 12;

/// What the scripted server says to one request.
#[derive(Debug, Clone)]
pub enum Reply {
    Accept,
    Unauthorized,
    Forbidden(Option<&'static str>),
    Closed,
}

/// One request as the inner client received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub method: &'static str,
    pub uri: String,
    pub session: Option<String>,
    pub last_event_id: Option<String>,
    pub token: Option<String>,
    pub headers: usize,
    pub limit: Option<usize>,
}

/// An inner client that replies from a queue (then accepts) and keeps a log.
#[derive(Debug, Clone)]
pub struct Scripted {
    challenge: String,
    replies: Arc<Mutex<VecDeque<Reply>>>,
    /// What to say once the queue is empty.
    then: Reply,
    calls: Arc<Mutex<Vec<Call>>>,
}

impl Scripted {
    pub fn new(site: &Site, replies: &[Reply], then: Reply) -> Self {
        Self {
            challenge: site.challenge(),
            replies: Arc::new(Mutex::new(replies.iter().cloned().collect())),
            then,
            calls: Arc::default(),
        }
    }

    fn answer(&self, call: Call) -> Result<(), Failure> {
        let sent = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(call);
            calls.len()
        };
        // A client that never stops resending is cut off with a failure no
        // test expects, so a broken retry limit fails a test instead of
        // hanging it.
        if sent > RUNAWAY {
            return Err(StreamableHttpError::UnexpectedEndOfStream);
        }
        let reply = self.replies.lock().unwrap().pop_front();
        match reply.unwrap_or_else(|| self.then.clone()) {
            Reply::Accept => Ok(()),
            Reply::Unauthorized => Err(StreamableHttpError::AuthRequired(AuthRequiredError::new(
                self.challenge.clone(),
            ))),
            Reply::Forbidden(scope) => Err(StreamableHttpError::InsufficientScope(
                InsufficientScopeError::new("Bearer error=x".to_owned(), scope.map(str::to_owned)),
            )),
            Reply::Closed => Err(StreamableHttpError::TransportChannelClosed),
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub fn tokens(&self) -> Vec<Option<String>> {
        self.calls().into_iter().map(|call| call.token).collect()
    }
}

pub type Headers = HashMap<HeaderName, HeaderValue>;
type EventStream = BoxStream<'static, Result<Sse, SseError>>;

fn call(method: &'static str, uri: &Arc<str>, token: Option<String>, headers: &Headers) -> Call {
    Call {
        method,
        uri: uri.to_string(),
        session: None,
        last_event_id: None,
        token,
        headers: headers.len(),
        limit: None,
    }
}

impl StreamableHttpClient for Scripted {
    type Error = std::io::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        _message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: Headers,
    ) -> Result<StreamableHttpPostResponse, Failure> {
        let mut call = call("post", &uri, auth_header, &custom_headers);
        call.session = session_id.map(|id| id.to_string());
        self.answer(call)?;
        Ok(StreamableHttpPostResponse::Accepted)
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        _message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: Headers,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, Failure> {
        let mut call = call("post-limited", &uri, auth_header, &custom_headers);
        call.session = session_id.map(|id| id.to_string());
        call.limit = Some(max_sse_event_size);
        self.answer(call)?;
        Ok(StreamableHttpPostResponse::Accepted)
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: Headers,
    ) -> Result<(), Failure> {
        let mut call = call("delete", &uri, auth_header, &custom_headers);
        call.session = Some(session_id.to_string());
        self.answer(call)
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: Headers,
    ) -> Result<EventStream, Failure> {
        let mut call = call("get", &uri, auth_header, &custom_headers);
        call.session = session_id.map(|id| id.to_string());
        call.last_event_id = last_event_id;
        self.answer(call)?;
        Ok(futures::stream::iter([Ok(Sse::default().data("one"))]).boxed())
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: Headers,
        max_sse_event_size: usize,
    ) -> Result<EventStream, Failure> {
        let mut call = call("get-limited", &uri, auth_header, &custom_headers);
        call.session = session_id.map(|id| id.to_string());
        call.last_event_id = last_event_id;
        call.limit = Some(max_sse_event_size);
        self.answer(call)?;
        Ok(futures::stream::iter([Ok(Sse::default().data("one"))]).boxed())
    }
}
