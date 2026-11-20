// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A Streamable HTTP endpoint behind the recorder's proxy, driven from this
//! process.
//!
//! The proxy is `mcp-trace-capture http`'s, in process, on a loopback port of
//! its own for the run; this client posts to it, and it forwards the bytes
//! unchanged and records them. TLS, where the server's URL is `https`, is the
//! proxy's, so this client speaks plain HTTP to loopback only.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, BodyDataStream};
use axum::http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue, Method, Request, Uri};
use futures::StreamExt as _;
use mcp_conformance_core::trace::PROBE_AUTHORIZATION;
use mcp_trace_capture::DEFAULT_MAX_MESSAGE;
use mcp_trace_capture::http::{self, Client, Options};
use mcp_trace_capture::output::{self, Target};
use mcp_trace_capture::run::HostHeader;
use mcp_trace_capture::traces::Traces;
use serde_json::Value;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::{Answer, Failure, Incoming, classify};
use crate::check::request::{Credential, Outgoing, Requests};
use crate::check::sse::Events;
use crate::proxy;

/// How long connecting to the proxy may take; it is on this machine.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The most times one answer's stream is resumed after the server closes it.
const MAX_RESUMPTIONS: usize = 3;

/// The longest wait before resuming, whatever `retry` the server asked for.
const MAX_RETRY_WAIT_MS: u64 = 2_000;

/// How the recorder's proxy opens the body of the `502` it answers with when
/// it cannot reach the URL (`mcp_trace_capture`'s `upstream_failed`); the URL
/// without its query and the cause follow.
const UNREACHABLE: &str = "mcp-trace-capture: cannot reach the upstream ";

const SESSION_ID: &str = "mcp-session-id";
const PROTOCOL_VERSION: &str = "mcp-protocol-version";

/// The proxy and a client pointed at it.
#[derive(Debug)]
pub(crate) struct Http {
    client: Client,
    /// The server's path and query, on the proxy.
    endpoint: Uri,
    /// Headers the user asked for on every request (`--header`).
    extra: Vec<(HeaderName, HeaderValue)>,
    /// The session the server assigned (legacy era), and the revision header
    /// last sent, for the closing `DELETE`.
    session: Option<String>,
    revision: Option<String>,
    stop: oneshot::Sender<()>,
    serving: JoinHandle<std::io::Result<http::Unrecorded>>,
    traces: Arc<Traces>,
}

impl Http {
    /// Starts the proxy in front of `url`, recording to `target`.
    ///
    /// # Errors
    ///
    /// `url` is not an absolute `http(s)` URL the proxy accepts, the trace
    /// cannot be created, or no loopback port can be bound. The message never
    /// repeats the URL, whose query can carry a key.
    pub(crate) async fn start(
        url: &str,
        extra: Vec<(HeaderName, HeaderValue)>,
        target: &Target,
        force: bool,
    ) -> Result<(Self, PathBuf), String> {
        let (upstream, path) = proxy::split(url).map_err(|error| format!("--url: {error}"))?;
        let mut options = Options::new(upstream.clone(), DEFAULT_MAX_MESSAGE)
            .map_err(|error| format!("--url: {error}"))?;
        // The recorder's own default: the client's `Host` for a server on this
        // machine, the server's own name for any other.
        options.preserve_host = HostHeader::Auto.preserves(http::is_loopback(&upstream));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| format!("cannot listen on 127.0.0.1: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot read the listening address: {error}"))?;
        let endpoint: Uri = format!("http://{address}{path}")
            .parse()
            .map_err(|_| "--url: its path cannot be requested".to_owned())?;
        let client = http::client(&endpoint, CONNECT_TIMEOUT)?;
        let (traces, file) = output::traces(target, force, DEFAULT_MAX_MESSAGE, None)?;
        let file = file.ok_or_else(|| "the trace needs a file name".to_owned())?;
        let (stop, stopped) = oneshot::channel::<()>();
        let serving = tokio::spawn(http::serve_traces(
            listener,
            Arc::clone(&traces),
            options,
            async {
                let _ = stopped.await;
            },
        ));
        let http = Self {
            client,
            endpoint,
            extra,
            session: None,
            revision: None,
            stop,
            serving,
            traces,
        };
        Ok((http, file))
    }

    fn request(
        &mut self,
        method: Method,
        out: Option<&Outgoing>,
    ) -> Result<Request<Body>, Failure> {
        let mut request = Request::builder()
            .method(method)
            .uri(self.endpoint.clone())
            .header(ACCEPT, "application/json, text/event-stream");
        if out.is_some() {
            request = request.header(CONTENT_TYPE, "application/json");
        }
        if let Some(session) = &self.session {
            request = request.header(SESSION_ID, session);
        }
        for (name, value) in out.map(|out| out.headers.as_slice()).unwrap_or_default() {
            if name.eq_ignore_ascii_case(PROTOCOL_VERSION) {
                self.revision = Some(value.clone());
            }
            request = request.header(name, value);
        }
        if out.is_none()
            && let Some(revision) = &self.revision
        {
            request = request.header(PROTOCOL_VERSION, revision);
        }
        let credential = out.map_or(Credential::Given, |out| out.credential);
        for (name, value) in &self.extra {
            if credential != Credential::Given && name == AUTHORIZATION {
                continue;
            }
            request = request.header(name, value);
        }
        if credential == Credential::Probe {
            request = request.header(AUTHORIZATION, PROBE_AUTHORIZATION);
        }
        let body = out.map_or_else(Body::empty, |out| Body::from(out.body.to_string()));
        request
            .body(body)
            .map_err(|error| Failure::Transport(format!("cannot build the request: {error}")))
    }

    async fn send(
        &mut self,
        method: Method,
        out: Option<&Outgoing>,
    ) -> Result<axum::http::Response<Body>, Failure> {
        let request = self.request(method, out)?;
        match self.client.request(request).await {
            Ok(response) => Ok(response.map(Body::new)),
            Err(error) => Err(Failure::Transport(format!(
                "the request did not reach the recording proxy: {error}"
            ))),
        }
    }

    /// Whether the user gave a credential (`--header 'Authorization: …'`).
    pub(crate) fn has_credential(&self) -> bool {
        self.extra.iter().any(|(name, _)| name == AUTHORIZATION)
    }

    /// Posts the notification (or the answer to a server's request) `out`.
    pub(crate) async fn notify(&mut self, out: &Outgoing) -> Result<(), Failure> {
        // The status is the server's to get right (202) and the validator's
        // to judge; the client has nothing to read back.
        self.send(Method::POST, Some(out)).await.map(|_| ())
    }

    /// Posts `out` and reads its answer: one JSON body, or an event stream
    /// read until the answer arrives.
    pub(crate) async fn exchange(
        &mut self,
        out: &Outgoing,
        requests: &Requests,
        max_response: usize,
    ) -> Result<Answer, Failure> {
        let response = self.send(Method::POST, Some(out)).await?;
        let status = response.status().as_u16();
        let session = response
            .headers()
            .get(SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let streamed = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|kind| {
                kind.split(';')
                    .next()
                    .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("text/event-stream"))
            });
        let body = response.into_body().into_data_stream();
        if streamed {
            self.stream(body, out, requests, max_response, session)
                .await
        } else {
            let whole = whole(body, max_response).await?;
            match serde_json::from_slice::<Value>(&whole).map(|message| classify(message, out.id)) {
                Ok(Incoming::Answer(message)) => Ok(Answer {
                    message,
                    session,
                    server_requests: 0,
                }),
                // The tap's own 502: nothing answered at the URL, so there
                // is no server to hold the status against.
                _ if status == 502 && whole.starts_with(UNREACHABLE.as_bytes()) => {
                    let cause = String::from_utf8_lossy(&whole[UNREACHABLE.len()..]);
                    Err(Failure::Transport(format!(
                        "cannot reach the server at {}",
                        cause.trim()
                    )))
                }
                // An error the server could not tie to the request (`id:
                // null`), or a status with a body that is not JSON-RPC at all.
                _ => Err(Failure::Http(status)),
            }
        }
    }

    /// Reads an event stream until the answer to `out`, answering any request
    /// the server sends on it (the legacy era allows one there).
    async fn stream(
        &mut self,
        mut body: BodyDataStream,
        out: &Outgoing,
        requests: &Requests,
        max_response: usize,
        session: Option<String>,
    ) -> Result<Answer, Failure> {
        let mut events = Events::default();
        let mut read = 0_usize;
        let mut server_requests = 0;
        let mut resumed = 0;
        loop {
            while let Some(chunk) = body.next().await {
                let chunk = chunk
                    .map_err(|error| Failure::Transport(format!("reading the answer: {error}")))?;
                read = read.saturating_add(chunk.len());
                if read > max_response {
                    return Err(Failure::TooLarge);
                }
                for data in events.feed(&chunk) {
                    let Ok(message) = serde_json::from_str::<Value>(&data) else {
                        continue;
                    };
                    match classify(message, out.id) {
                        Incoming::Answer(message) => {
                            return Ok(Answer {
                                message,
                                session,
                                server_requests,
                            });
                        }
                        Incoming::Request(request) => {
                            server_requests += 1;
                            self.notify(&requests.answer(&request)).await?;
                        }
                        Incoming::Other => {}
                    }
                }
            }
            // The stream ended before the answer. A server may close it and
            // deliver the rest when the client comes back for it.
            resumed += 1;
            match self.resume(&events, resumed).await? {
                Some(next) => body = next,
                None => return Err(Failure::Closed),
            }
        }
    }

    /// Comes back for a stream the server closed before the answer: a GET
    /// naming the last event received (`Last-Event-ID`), after the wait the
    /// server asked for (`retry`, capped). `None` when there is nothing to
    /// resume from — no event carried an id — when the server does not offer
    /// the stream again, or after [`MAX_RESUMPTIONS`].
    async fn resume(
        &mut self,
        events: &Events,
        attempt: usize,
    ) -> Result<Option<BodyDataStream>, Failure> {
        let Some(last) = events.last_id().filter(|_| attempt <= MAX_RESUMPTIONS) else {
            return Ok(None);
        };
        let wait = events.retry().unwrap_or(0).min(MAX_RETRY_WAIT_MS);
        tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
        let mut request = self.request(Method::GET, None)?;
        let Ok(last) = axum::http::HeaderValue::from_str(last) else {
            return Ok(None);
        };
        request.headers_mut().insert("last-event-id", last);
        let response = match self.client.request(request).await {
            Ok(response) => response.map(Body::new),
            Err(error) => {
                return Err(Failure::Transport(format!(
                    "the request did not reach the recording proxy: {error}"
                )));
            }
        };
        Ok(response
            .status()
            .is_success()
            .then(|| response.into_body().into_data_stream()))
    }

    /// Remembers the session the server assigned, for every later request.
    pub(crate) fn session(&mut self, session: Option<String>) {
        self.session = session;
    }

    /// Ends the session a legacy server assigned (`DELETE`, which it may
    /// refuse), stops the proxy and closes the trace.
    pub(crate) async fn close(mut self) -> Result<(), String> {
        if self.session.is_some() {
            let ending = self.send(Method::DELETE, None);
            let _ = tokio::time::timeout(CONNECT_TIMEOUT, ending).await;
        }
        let _ = self.stop.send(());
        self.serving
            .await
            .map_err(|error| format!("the recording proxy failed: {error}"))?
            .map_err(|error| format!("the recording proxy failed: {error}"))?;
        if self
            .traces
            .finish()
            .iter()
            .all(|finished| finished.summary.is_complete())
        {
            Ok(())
        } else {
            Err("a write to the trace failed, so it is incomplete".to_owned())
        }
    }
}

/// A response body read whole, within `max_response` bytes.
async fn whole(mut body: BodyDataStream, max_response: usize) -> Result<Vec<u8>, Failure> {
    let mut whole = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk =
            chunk.map_err(|error| Failure::Transport(format!("reading the answer: {error}")))?;
        if whole.len().saturating_add(chunk.len()) > max_response {
            return Err(Failure::TooLarge);
        }
        whole.extend_from_slice(&chunk);
    }
    Ok(whole)
}
