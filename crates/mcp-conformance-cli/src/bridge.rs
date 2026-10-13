// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The stdio bridge: how the suite, which drives servers only over streamable
//! HTTP, reaches a stdio server.
//!
//! It listens on `127.0.0.1` (an OS-assigned port, for the run only). Each
//! session the suite begins — an `initialize` POST without an `Mcp-Session-Id`
//! — starts one server process under the recorder's stdio wrapper
//! ([`mcp_trace_capture::stdio::relay`]), recording to the next numbered trace.
//! A message a `POST` carries is written to that process's stdin as one line: the body's
//! bytes, with any CR or LF in them (JSON whitespace between tokens; a JSON
//! string cannot hold one raw) turned into spaces — nothing else changes. What
//! the server writes goes back on the streams [`session`] describes. A `DELETE`
//! closes the server's stdin; a server still running two seconds later is
//! stopped as the wrapper stops one.
//!
//! The trace records the stdio wire, which is what the server spoke. The
//! bridge is an adapter for the suite inside `test`, not a gateway (ADR-0019):
//! it is not offered on its own, adds no routing or policy, and refuses any
//! request whose `Host` is not its own loopback address or whose `Origin` is
//! not loopback — a web page cannot drive the server through it.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::BufWriter;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use mcp_conformance_core::trace::DEFAULT_MAX_LINE_BYTES;
use mcp_trace_capture::numbered::Numbered;
use mcp_trace_capture::{DEFAULT_MAX_MESSAGE, Recorder, Signals, Stop, StopRequests, stdio};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

mod session;

use session::Session;

/// How long a server gets to exit after its stdin closes before it is
/// stopped — the official SDKs' stdio clients wait the same.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// The session header.
const SESSION_ID: &str = "mcp-session-id";

/// A running bridge.
#[derive(Debug)]
pub(crate) struct Bridge {
    /// Where it listens.
    pub(crate) address: SocketAddr,
    stop: oneshot::Sender<()>,
    serving: JoinHandle<()>,
    state: Arc<State>,
}

#[derive(Debug)]
struct State {
    program: OsString,
    args: Vec<OsString>,
    numbered: Numbered,
    /// The `Host` values the bridge answers to.
    hosts: [String; 2],
    sessions: Mutex<HashMap<String, Live>>,
    /// Every session's relay, for the final wait.
    relays: Mutex<Vec<JoinHandle<()>>>,
    next: AtomicU64,
}

#[derive(Debug, Clone)]
struct Live {
    session: Arc<Session>,
    stop: StopRequests,
}

/// Starts a bridge to `command` (split at whitespace), recording each session
/// to the next `NNN.jsonl` in `dir`. The command is started once and stopped
/// first, so one that cannot start fails here rather than in every scenario.
///
/// # Errors
///
/// The command is empty or cannot be started, or the listener cannot bind.
pub(crate) async fn start(command: &str, dir: &Path) -> Result<Bridge, String> {
    let mut words = command.split_whitespace().map(OsString::from);
    let program = words
        .next()
        .ok_or_else(|| "--command is empty".to_owned())?;
    let args: Vec<OsString> = words.collect();
    stdio::spawn(&program, &args)
        .map_err(|error| format!("cannot start the server {}: {error}", program.display()))?
        .kill()
        .await;
    let numbered = Numbered::parse(&dir.join("{session}.jsonl"))?
        .ok_or_else(|| format!("{}: cannot number traces there", dir.display()))?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("cannot listen on 127.0.0.1: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("cannot read the listening address: {error}"))?;
    let state = Arc::new(State {
        program,
        args,
        numbered,
        hosts: [
            format!("127.0.0.1:{}", address.port()),
            format!("localhost:{}", address.port()),
        ],
        sessions: Mutex::new(HashMap::new()),
        relays: Mutex::new(Vec::new()),
        next: AtomicU64::new(1),
    });
    let app = axum::Router::new().fallback({
        let state = Arc::clone(&state);
        move |request: Request<Body>| handle(Arc::clone(&state), request)
    });
    let (stop, stopped) = oneshot::channel::<()>();
    let serving = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    Ok(Bridge {
        address,
        stop,
        serving,
        state,
    })
}

impl Bridge {
    /// Ends every session as a `DELETE` would, waits for their servers, and
    /// stops listening.
    pub(crate) async fn stop(self) {
        let live: Vec<Live> = self
            .state
            .lock_sessions()
            .drain()
            .map(|(_, live)| live)
            .collect();
        for live in live {
            end(live).await;
        }
        let relays: Vec<JoinHandle<()>> = self
            .state
            .relays
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        for relay in relays {
            let _ = relay.await;
        }
        let _ = self.stop.send(());
        // Event streams still held open by the suite end with their sessions;
        // anything else gets a moment, not forever.
        let _ = tokio::time::timeout(Duration::from_secs(5), self.serving).await;
    }
}

impl State {
    fn lock_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, Live>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The session a request names, or the status and text refusing it.
    fn session(&self, headers: &HeaderMap) -> Result<Live, (StatusCode, &'static str)> {
        let id = headers
            .get(SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .ok_or((StatusCode::BAD_REQUEST, "Bad Request: no Mcp-Session-Id"))?;
        self.lock_sessions()
            .get(id)
            .cloned()
            .ok_or((StatusCode::NOT_FOUND, "Not Found: no such session"))
    }

    /// Starts a server process and its session.
    fn begin(&self) -> Result<(String, Live), String> {
        let server = stdio::spawn(&self.program, &self.args)
            .map_err(|error| format!("cannot start the server: {error}"))?;
        let (_, path, file) = self
            .numbered
            .create_next(1)
            .map_err(|error| format!("cannot create a trace: {error}"))?;
        let recorder = Arc::new(Recorder::with_max_line(
            BufWriter::new(file),
            DEFAULT_MAX_LINE_BYTES,
        ));
        let (writer, from_client) = tokio::io::duplex(64 * 1024);
        let (to_client, answers) = tokio::io::duplex(64 * 1024);
        let (signals, stop) = Signals::requested();
        let session = Arc::new(Session::new(writer));
        tokio::spawn(session::route_output(Arc::clone(&session), answers));
        let relay = tokio::spawn(async move {
            let outcome = stdio::relay(
                Arc::clone(&recorder),
                server,
                signals,
                from_client,
                to_client,
                DEFAULT_MAX_MESSAGE,
            )
            .await;
            if let Err(error) = outcome {
                eprintln!("mcp-conformance: waiting for the stdio server failed: {error}");
            }
            if !recorder.finish().is_complete() {
                eprintln!(
                    "mcp-conformance: a write to {} failed; the trace is incomplete",
                    path.display()
                );
            }
        });
        self.relays
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(relay);
        let id = format!(
            "stdio-bridge-{}-{}",
            self.next.fetch_add(1, Ordering::Relaxed),
            std::process::id()
        );
        let live = Live { session, stop };
        self.lock_sessions().insert(id.clone(), live.clone());
        Ok((id, live))
    }

    /// Whether the request's `Host` is the bridge's own and its `Origin`, if
    /// any, is loopback.
    fn trusted(&self, headers: &HeaderMap) -> bool {
        let host = headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok());
        let host_ok = host.is_some_and(|host| self.hosts.iter().any(|ours| ours == host));
        let origin_ok = headers.get(header::ORIGIN).is_none_or(|origin| {
            origin.to_str().is_ok_and(|origin| {
                ["http://127.0.0.1", "http://localhost"]
                    .iter()
                    .any(|loopback| {
                        origin == *loopback
                            || origin
                                .strip_prefix(loopback)
                                .is_some_and(|rest| rest.starts_with(':'))
                    })
            })
        });
        host_ok && origin_ok
    }
}

/// Closes a session's stdin, and stops its server if it has not exited within
/// [`CLOSE_GRACE`].
async fn end(live: Live) {
    live.session.close().await;
    let stop = live.stop;
    tokio::spawn(async move {
        tokio::time::sleep(CLOSE_GRACE).await;
        stop.request(Stop::Terminate);
    });
}

async fn handle(state: Arc<State>, request: Request<Body>) -> Response {
    if !state.trusted(request.headers()) {
        return plain(StatusCode::FORBIDDEN, "Forbidden: not a loopback request");
    }
    let method = request.method().clone();
    if method == Method::POST {
        post(&state, request).await
    } else if method == Method::GET {
        match state.session(request.headers()) {
            Ok(live) => events(live.session.open_standalone(), None),
            Err(refusal) => refusal.into_response(),
        }
    } else if method == Method::DELETE {
        match state.session(request.headers()) {
            Ok(live) => {
                if let Some(id) = request
                    .headers()
                    .get(SESSION_ID)
                    .and_then(|id| id.to_str().ok())
                {
                    state.lock_sessions().remove(id);
                }
                end(live).await;
                StatusCode::OK.into_response()
            }
            Err(refusal) => refusal.into_response(),
        }
    } else {
        plain(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed")
    }
}

async fn post(state: &State, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(body) = axum::body::to_bytes(body, DEFAULT_MAX_MESSAGE).await else {
        return plain(StatusCode::PAYLOAD_TOO_LARGE, "Payload Too Large");
    };
    let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return parse_error();
    };
    let (live, new_id) = if parts.headers.contains_key(SESSION_ID) {
        match state.session(&parts.headers) {
            Ok(live) => (live, None),
            Err(refusal) => return refusal.into_response(),
        }
    } else if is_initialize(&message) {
        match state.begin() {
            Ok((id, live)) => (live, Some(id)),
            Err(error) => {
                eprintln!("mcp-conformance: {error}");
                return plain(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the stdio server did not start",
                );
            }
        }
    } else {
        return plain(StatusCode::BAD_REQUEST, "Bad Request: no Mcp-Session-Id");
    };
    let ids = session::request_ids(&message);
    // Opened before the line is written, so no response can outrun its stream.
    let stream = (!ids.is_empty()).then(|| live.session.open_stream(ids));
    if live.session.send(&line(&body)).await.is_err() {
        return plain(StatusCode::BAD_GATEWAY, "the stdio server has exited");
    }
    stream.map_or_else(
        || StatusCode::ACCEPTED.into_response(),
        |stream| events(stream, new_id),
    )
}

/// A POST body as one stdio line: CR and LF become spaces, then a newline.
fn line(body: &Bytes) -> Vec<u8> {
    let mut line: Vec<u8> = body
        .iter()
        .map(|&byte| {
            if byte == b'\n' || byte == b'\r' {
                b' '
            } else {
                byte
            }
        })
        .collect();
    line.push(b'\n');
    line
}

fn is_initialize(message: &serde_json::Value) -> bool {
    message.get("method").and_then(serde_json::Value::as_str) == Some("initialize")
        && message.get("id").is_some()
}

/// An event stream carrying each line as one `message` event.
fn events(
    receiver: tokio::sync::mpsc::UnboundedReceiver<String>,
    session_id: Option<String>,
) -> Response {
    let stream = futures::stream::unfold(receiver, |mut receiver| async move {
        let line = receiver.recv().await?;
        let event = Bytes::from(format!("event: message\ndata: {line}\n\n"));
        Some((Ok::<_, std::convert::Infallible>(event), receiver))
    });
    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );
    if let Some(id) = session_id.and_then(|id| header::HeaderValue::from_str(&id).ok()) {
        headers.insert(SESSION_ID, id);
    }
    response
}

fn plain(status: StatusCode, text: &'static str) -> Response {
    (status, text).into_response()
}

fn parse_error() -> Response {
    let body = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}"#;
    let mut response = (StatusCode::BAD_REQUEST, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
