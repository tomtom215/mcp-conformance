// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Session trace tap for the streamable HTTP transport (feature `tap`).
//!
//! Records every admitted MCP session as a validator-ready JSON Lines trace
//! ([`mcp_conformance_core::trace`]), one file per session, so the agreement
//! check (docs/plan/03-conformance-strategy.md §Calibration) can replay the
//! exact sessions the official runner drove and diff verdicts.
//!
//! Design rules:
//!
//! - **Pass-through fidelity.** The tap observes; it never alters the bytes,
//!   status, or headers of the proxied exchange. A recording failure is
//!   reported to stderr and the exchange continues untapped.
//! - **Redaction by construction.** Only the conformance-relevant headers in
//!   [`RECORDED_HEADERS`] are recorded; everything else (including any
//!   credential-bearing header) is never captured in the first place.
//! - **Sessions only.** The tap sits inside the security-policy layer, so
//!   policy rejections (403s) never reach it — they never form sessions and
//!   are the runner's and the corpus's concern, not the tap's.
//! - **Write-behind, in order.** Events flow over a bounded channel to one
//!   writer task that appends each line and flushes before taking the next,
//!   so a completed exchange is durable even if the process is killed before
//!   orderly shutdown. The writer assigns `seq` per file in arrival order,
//!   making the schema's strictly-increasing-seq rule hold by construction
//!   even when a session's POST exchanges and SSE streams record
//!   concurrently.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request};
use axum::middleware::Next;
use axum::response::Response;
use mcp_conformance_core::trace::{
    Direction, EventBody, LifecycleEvent, TraceEvent, TransportKind,
};
use tokio::io::AsyncWriteExt as _;
use tokio_stream::StreamExt as _;

/// Headers recorded into traces (lowercase). Everything absent from this
/// allowlist — notably `authorization` and `cookie` — is never captured.
const RECORDED_HEADERS: [&str; 7] = [
    "host",
    "origin",
    "accept",
    "content-type",
    "mcp-session-id",
    "mcp-protocol-version",
    "last-event-id",
];

/// The session-id header of the streamable HTTP transport (`2025-11-25`
/// basic/transports §session management).
const SESSION_ID_HEADER: &str = "mcp-session-id";

/// Largest request/response body the tap will buffer for recording. The
/// suite's payloads are kilobytes; anything larger is passed through
/// unrecorded with a stderr note rather than held in memory.
const MAX_RECORDED_BODY: usize = 4 * 1024 * 1024;

/// Capacity of the event channel to the writer task. Sending applies
/// backpressure to the recorded exchange rather than dropping events: a
/// trace with holes is worse than a slightly slower proxied response.
const CHANNEL_CAPACITY: usize = 1024;

/// One recordable moment, routed to the writer task (which sequences it).
struct Record {
    /// Tap-assigned session file key.
    file: Arc<SessionFile>,
    /// Which party emitted the event.
    direction: Direction,
    /// The event body.
    body: EventBody,
}

/// Identity of one session's trace file.
struct SessionFile {
    /// Final path of the JSON Lines trace.
    path: PathBuf,
}

/// Shared tap state installed into the middleware.
pub struct Tap {
    dir: PathBuf,
    sender: tokio::sync::mpsc::Sender<Record>,
    sessions: Mutex<HashMap<String, Arc<SessionFile>>>,
    created: AtomicU64,
}

impl std::fmt::Debug for Tap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tap")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Tap {
    /// Creates the tap and spawns its writer task. The directory is created
    /// eagerly so misconfiguration fails at startup, not mid-session.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the trace directory cannot be created.
    pub fn new(dir: PathBuf) -> std::io::Result<Arc<Self>> {
        std::fs::create_dir_all(&dir)?;
        let (sender, receiver) = tokio::sync::mpsc::channel(CHANNEL_CAPACITY);
        tokio::spawn(write_loop(receiver));
        Ok(Arc::new(Self {
            dir,
            sender,
            sessions: Mutex::new(HashMap::new()),
            created: AtomicU64::new(0),
        }))
    }

    /// The trace file for `session_id`, creating its identity on first sight.
    fn session(&self, session_id: &str) -> Arc<SessionFile> {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(file) = sessions.get(session_id) {
            return Arc::clone(file);
        }
        let ordinal = self.created.fetch_add(1, Ordering::Relaxed) + 1;
        // The ordinal keeps directory listings in session-creation order; the
        // id makes the session ↔ trace correspondence self-describing.
        let path = self.dir.join(format!("{ordinal:03}-{session_id}.jsonl"));
        let file = Arc::new(SessionFile { path });
        sessions.insert(session_id.to_owned(), Arc::clone(&file));
        file
    }

    /// Enqueues one event for `session_id`; the writer task sequences it.
    /// On channel pressure this awaits (the exchange slows; the trace stays
    /// whole).
    async fn record(&self, session_id: &str, direction: Direction, body: EventBody) {
        let file = self.session(session_id);
        if self
            .sender
            .send(Record {
                file,
                direction,
                body,
            })
            .await
            .is_err()
        {
            eprintln!("mcp-everything-server: tap writer gone; event dropped");
        }
    }
}

/// Per-file writer state: the open handle and the next sequence number.
struct FileState {
    file: tokio::fs::File,
    next_seq: u64,
}

/// The writer task: sequences each record per file (the schema's
/// strictly-increasing rule holds by construction), appends it as one JSON
/// line, and flushes before accepting the next — everything enqueued before
/// a kill is durable.
async fn write_loop(mut receiver: tokio::sync::mpsc::Receiver<Record>) {
    let mut files: HashMap<PathBuf, FileState> = HashMap::new();
    while let Some(record) = receiver.recv().await {
        let path = &record.file.path;
        if !files.contains_key(path) {
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await
            {
                Ok(file) => {
                    files.insert(path.clone(), FileState { file, next_seq: 0 });
                }
                Err(error) => {
                    eprintln!(
                        "mcp-everything-server: tap cannot open {}: {error}",
                        path.display()
                    );
                    continue;
                }
            }
        }
        if let Some(state) = files.get_mut(path) {
            let event = TraceEvent::new(
                state.next_seq,
                record.direction,
                TransportKind::StreamableHttp,
                record.body,
            );
            let Ok(line) = serde_json::to_string(&event) else {
                eprintln!("mcp-everything-server: tap event unserializable; skipped");
                continue;
            };
            state.next_seq += 1;
            let write = async {
                state.file.write_all(line.as_bytes()).await?;
                state.file.write_all(b"\n").await?;
                state.file.flush().await
            };
            if let Err(error) = write.await {
                eprintln!(
                    "mcp-everything-server: tap write to {} failed: {error}",
                    path.display()
                );
            }
        }
    }
}

/// The middleware: records the request exchange and, for SSE responses,
/// every streamed frame, attributing everything to the exchange's session.
pub async fn tap_layer(
    State(tap): State<Arc<Tap>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let request_headers = recorded_headers(request.headers());
    let request_session = header_value(request.headers(), SESSION_ID_HEADER);

    // Buffer the request body for recording, then reconstruct the request.
    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_RECORDED_BODY).await else {
        // Body larger than the recording cap (or unreadable): the tap
        // steps aside entirely rather than guess at fidelity.
        eprintln!("mcp-everything-server: tap skipped an oversized request body");
        return next.run(Request::from_parts(parts, Body::empty())).await;
    };
    let request_payload: Option<serde_json::Value> = serde_json::from_slice(&bytes).ok();
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes.clone())))
        .await;

    // The session this exchange belongs to: the response names it on
    // initialize; every later exchange names it on the request.
    let response_session = header_value(response.headers(), SESSION_ID_HEADER);
    let Some(session_id) = response_session.or(request_session) else {
        return response; // Sessionless exchange (e.g. rejected init): out of tap scope.
    };

    tap.record(
        &session_id,
        Direction::ClientToServer,
        EventBody::Http {
            status: None,
            headers: request_headers,
        },
    )
    .await;
    if let Some(payload) = request_payload {
        tap.record(
            &session_id,
            Direction::ClientToServer,
            EventBody::Message { payload },
        )
        .await;
    }

    record_response(&tap, &session_id, method, response).await
}

/// Records the response side of one exchange: the HTTP observation, the
/// transport-close moment on session DELETE, and the body's message(s) —
/// streamed frame-by-frame for SSE, buffered for JSON.
async fn record_response(
    tap: &Arc<Tap>,
    session_id: &str,
    method: Method,
    response: Response,
) -> Response {
    let status = response.status();
    let content_type = header_value(response.headers(), "content-type");
    tap.record(
        session_id,
        Direction::ServerToClient,
        EventBody::Http {
            status: Some(status.as_u16()),
            headers: recorded_headers(response.headers()),
        },
    )
    .await;

    // Session teardown is a transport-close moment.
    if method == Method::DELETE && status.is_success() {
        tap.record(
            session_id,
            Direction::ServerToClient,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose,
            },
        )
        .await;
        return response;
    }

    if content_type
        .as_deref()
        .is_some_and(|v| v.starts_with("text/event-stream"))
    {
        return record_sse(tap, session_id, response);
    }
    if content_type
        .as_deref()
        .is_some_and(|v| v.starts_with("application/json"))
    {
        return record_json(tap, session_id, response).await;
    }
    response
}

/// Re-bodies an SSE response with a recording pass-through: frames are
/// recorded as they flow, bytes are forwarded untouched.
fn record_sse(tap: &Arc<Tap>, session_id: &str, response: Response) -> Response {
    let (parts, body) = response.into_parts();
    let tap = Arc::clone(tap);
    let session = session_id.to_owned();
    let mut splitter = SseSplitter::default();
    let stream = body.into_data_stream().then(move |chunk| {
        let tap = Arc::clone(&tap);
        let session = session.clone();
        let payloads = chunk
            .as_ref()
            .map_or_else(|_| Vec::new(), |bytes| splitter.push(bytes));
        async move {
            for payload in payloads {
                tap.record(
                    &session,
                    Direction::ServerToClient,
                    EventBody::Message { payload },
                )
                .await;
            }
            chunk
        }
    });
    Response::from_parts(parts, Body::from_stream(stream))
}

/// Buffers, records, and re-bodies a JSON response.
async fn record_json(tap: &Arc<Tap>, session_id: &str, response: Response) -> Response {
    let (parts, body) = response.into_parts();
    if let Ok(bytes) = axum::body::to_bytes(body, MAX_RECORDED_BODY).await {
        if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            tap.record(
                session_id,
                Direction::ServerToClient,
                EventBody::Message { payload },
            )
            .await;
        }
        Response::from_parts(parts, Body::from(bytes))
    } else {
        eprintln!("mcp-everything-server: tap lost an oversized response body");
        Response::from_parts(parts, Body::empty())
    }
}

/// The allowlisted subset of `headers`, lowercased.
fn recorded_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    RECORDED_HEADERS
        .iter()
        .filter_map(|name| header_value(headers, name).map(|value| ((*name).to_owned(), value)))
        .collect()
}

/// A header's value as UTF-8, when present and decodable.
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

/// Incremental SSE frame splitter: feed byte chunks, get the JSON payloads
/// of completed `data:` frames. Carries partial frames across chunks.
#[derive(Default)]
struct SseSplitter {
    buffer: String,
}

impl SseSplitter {
    /// Consumes one chunk and returns the payloads of every frame it
    /// completed. Non-UTF-8 chunks abort recording for this stream (the
    /// bytes still flow to the client untouched).
    fn push(&mut self, chunk: &[u8]) -> Vec<serde_json::Value> {
        let Ok(text) = std::str::from_utf8(chunk) else {
            self.buffer.clear();
            return Vec::new();
        };
        self.buffer.push_str(text);
        let mut payloads = Vec::new();
        // SSE events end at a blank line; tolerate both LF and CRLF framing.
        while let Some((frame, rest)) = split_frame(&self.buffer) {
            let data = frame
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|d| d.strip_prefix(' ').unwrap_or(d))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty()
                && let Ok(payload) = serde_json::from_str(&data)
            {
                payloads.push(payload);
            }
            self.buffer = rest;
        }
        payloads
    }
}

/// Splits `buffer` at the first SSE frame boundary (`\n\n` or `\r\n\r\n`),
/// returning the frame and the remainder.
fn split_frame(buffer: &str) -> Option<(String, String)> {
    let lf = buffer.find("\n\n").map(|i| (i, 2));
    let crlf = buffer.find("\r\n\r\n").map(|i| (i, 4));
    let (index, width) = match (lf, crlf) {
        (Some((li, lw)), Some((ci, cw))) => {
            if ci < li {
                (ci, cw)
            } else {
                (li, lw)
            }
        }
        (Some(found), None) | (None, Some(found)) => found,
        (None, None) => return None,
    };
    Some((
        buffer[..index].to_owned(),
        buffer[index + width..].to_owned(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn splitter_yields_each_completed_frame_and_carries_partials() {
        let mut splitter = SseSplitter::default();
        assert!(splitter.push(b"data: {\"a\":").is_empty());
        let got = splitter.push(b"1}\n\ndata: {\"b\":2}\n\ndata: {\"c\"");
        assert_eq!(got, vec![json!({"a": 1}), json!({"b": 2})]);
        assert_eq!(splitter.push(b":3}\n\n"), vec![json!({"c": 3})]);
    }

    #[test]
    fn splitter_joins_multi_line_data_and_tolerates_crlf() {
        let mut splitter = SseSplitter::default();
        let got = splitter.push(b"event: message\r\ndata: [1,\r\ndata: 2]\r\n\r\n");
        assert_eq!(got, vec![json!([1, 2])]);
    }

    #[test]
    fn splitter_ignores_non_json_and_empty_frames() {
        let mut splitter = SseSplitter::default();
        assert!(splitter.push(b": keep-alive\n\n").is_empty());
        assert!(splitter.push(b"data: not json\n\n").is_empty());
        assert_eq!(splitter.push(b"data: 7\n\n"), vec![json!(7)]);
    }

    #[test]
    fn recorded_headers_is_an_allowlist() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        headers.insert("cookie", "id=1".parse().unwrap());
        headers.insert("host", "localhost:1234".parse().unwrap());
        headers.insert("mcp-session-id", "abc".parse().unwrap());
        let recorded = recorded_headers(&headers);
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded["host"], "localhost:1234");
        assert_eq!(recorded["mcp-session-id"], "abc");
        assert!(!recorded.contains_key("authorization"));
    }
}
