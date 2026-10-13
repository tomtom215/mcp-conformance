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
//! - **Every admitted exchange, session or not.** The tap sits inside the
//!   security-policy layer, so policy rejections (403s) never reach it. What
//!   does reach it is recorded whether or not it names a session: `2026-07-28`
//!   removes sessions outright, and an exchange with no `Mcp-Session-Id` goes
//!   to the run's `stateless` trace file rather than being dropped. One naming
//!   a session the server never assigned goes to the run's `unassigned` trace
//!   ([`UNASSIGNED_TRACE`]).
//! - **Write-behind, in order.** Records flow over a bounded channel to one
//!   writer task that appends each record's events as adjacent lines and
//!   flushes before taking the next; a request and its message are one record
//!   (see `Record`).
//!   Durability is per *written* record: everything the writer has flushed
//!   survives a kill; records still queued die with the process, and a kill
//!   mid-append can leave one torn final line (consumers treat an
//!   unparseable final line as in-progress, not corruption — pinned by
//!   `tap_survives_sigkill_with_a_parseable_prefix` in tests/cli.rs). The
//!   writer assigns `seq` per file in the order records reach it, making the
//!   schema's strictly-increasing-seq rule hold by construction even when a
//!   session's POST exchanges and SSE streams record concurrently; `xtask
//!   conformance` waits for tap quiescence before stopping the server, so gate
//!   runs read complete traces.
//! - **`seq` is observation order.** A request is sent to the writer when the
//!   tap has read it, before the service sees it — not once the service has
//!   answered, which put two overlapping requests in the order of their
//!   answers, and a notification the request caused ahead of the request. A
//!   request naming no session cannot know its file yet (its response may
//!   assign a session), so it reserves its place in the `stateless` trace
//!   and settles it once the response says; records behind an open place wait
//!   for it in the writer's memory, not on disk.
//! - **Sessions are trusted to be distinct.** Files are keyed by the
//!   server-assigned `Mcp-Session-Id` where there is one; a client that
//!   fabricates another session's ID would interleave into that session's file,
//!   and sessionless exchanges share one file per run because the wire offers
//!   no finer identity once sessions are gone. A session's state is kept for
//!   the run, `DELETE` included: releasing it there would split a session
//!   whose stream records after it, and send a request after teardown to the
//!   `unassigned` trace. The tap is diagnostics for runs
//!   this repository drives (the official runner over loopback), not a forensic
//!   recorder against adversarial clients — the security boundary lives in the
//!   policy layer in front of it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request};
use axum::middleware::Next;
use axum::response::Response;
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent};
use tokio_stream::StreamExt as _;

mod body;
mod headers;
mod sse;

use body::{Buffered, buffer};
use sse::SseSplitter;

/// The recording allowlist: everything absent from it — notably
/// `authorization` and `cookie` — is never written to a trace.
///
/// Defined by [`mcp_conformance_core::trace::RECORDED_HEADERS`], shared with
/// every capture tool so the recorders cannot drift apart; kept here as a
/// constant of its own (not a `pub use`) so this crate's API is unchanged for
/// the tools that read a crate's API from its own items.
pub const RECORDED_HEADERS: [&str; mcp_conformance_core::trace::RECORDED_HEADERS.len()] =
    mcp_conformance_core::trace::RECORDED_HEADERS;

/// Header-name prefixes recorded in addition to [`RECORDED_HEADERS`]; defined by
/// [`mcp_conformance_core::trace::RECORDED_HEADER_PREFIXES`].
pub const RECORDED_HEADER_PREFIXES: [&str;
    mcp_conformance_core::trace::RECORDED_HEADER_PREFIXES.len()] =
    mcp_conformance_core::trace::RECORDED_HEADER_PREFIXES;

/// The session-id header of the streamable HTTP transport (`2025-11-25`
/// basic/transports §session management).
const SESSION_ID_HEADER: &str = "mcp-session-id";

/// The trace identity for exchanges that carry no session id.
///
/// `2026-07-28` removes the session concept outright (SEP-2575) — every request
/// is standalone and carries its own context in `_meta` — so *every* exchange of
/// that revision arrives with no `Mcp-Session-Id`. Until 2026-08-17 the tap
/// dropped those on the floor and returned early, which made the capture path
/// structurally unable to record the revision at all, and did it **silently**:
/// the trace directory simply stayed empty, which is indistinguishable from a
/// server that was never talked to. That is the single reason the `2026-07-28`
/// corpus had to be hand-authored.
///
/// A `2025-11-25` exchange can be sessionless too — a rejected `initialize`, or
/// a request refused before a session existed — and those were being lost for
/// the same reason. They are exactly the exchanges a conformance capture wants.
///
/// The grouping is the server run, because once sessions are gone that is the
/// only identity the wire offers. Concurrent stateless clients therefore
/// interleave in one trace; the recording is still faithful (every event is
/// real and in capture order) and the reader is told so in `corpus/README.md`.
const STATELESS_TRACE: &str = "stateless";

/// The trace identity for exchanges naming a session the server never
/// assigned.
///
/// The service answers those 404 (`2025-11-25` basic/transports §session
/// management), and the exchange is evidence of that, so it is recorded — but
/// not under the name the client sent: until 2026-10-13 each made-up id opened a file
/// of its own, so a client could open files at will, and the agreement replay
/// judged each as a session that never initialized. One trace per run holds
/// them all; as with [`STATELESS_TRACE`], the run is the only grouping the
/// wire offers for them.
const UNASSIGNED_TRACE: &str = "unassigned";

/// Largest request/response body the tap will buffer for recording. The
/// suite's payloads are kilobytes; anything larger is passed through intact —
/// what was buffered, then the rest as it streams — unrecorded, with a stderr
/// note, rather than held in memory.
const MAX_RECORDED_BODY: usize = 4 * 1024 * 1024;

/// Capacity of the event channel to the writer task. Sending applies
/// backpressure to the recorded exchange rather than dropping events: a
/// trace with holes is worse than a slightly slower proxied response.
const CHANNEL_CAPACITY: usize = 1024;

/// Events routed to the writer task together: it sequences them and writes
/// them as adjacent lines, so nothing another exchange records can land
/// between them.
///
/// Adjacency is how the validator pairs a message with the HTTP event it rode:
/// a POST's message is read as the one after its request event, a JSON body as
/// the one after its status. Recorded with one send each, a concurrent GET's
/// request event could land between a POST and its message, and the message
/// was then judged to have ridden the GET (`TRAN-049`), as an official-suite
/// session in CI showed on 2026-10-12.
struct Record {
    /// The trace file the events belong to.
    file: Arc<SessionFile>,
    /// Each event and the party that emitted it, in order.
    events: Vec<(Direction, EventBody)>,
}

/// Shared tap state installed into the middleware.
///
/// Concurrency posture, re-decided 2026-06-13 (third audit): no `loom`
/// modeling. The shared state is a `Mutex<HashMap>` and one `AtomicU64`
/// counter — no hand-rolled lock-free structure for loom to catch out, and
/// `Relaxed` suffices for the counter because only its *uniqueness* is
/// load-bearing (`fetch_add` is atomic under any ordering). File names and
/// `seq` are the single writer task's. The interleaving evidence is the
/// 16-session real-parallelism stress test plus the SIGKILL durability test;
/// loom would re-prove std's Mutex, not this code.
pub struct Tap {
    dir: PathBuf,
    sender: tokio::sync::mpsc::Sender<Command>,
    /// Every trace seen this run. Sessions enter only when the server assigns
    /// them, on the response that carries the new `Mcp-Session-Id`.
    files: Mutex<HashMap<TraceKey, Arc<SessionFile>>>,
    /// Source of file identities and reserved-place numbers.
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
        tokio::spawn(write_loop(dir.clone(), receiver));
        Ok(Arc::new(Self {
            dir,
            sender,
            files: Mutex::new(HashMap::new()),
            created: AtomicU64::new(0),
        }))
    }

    /// The trace for `key`, creating its identity on first sight.
    fn file(&self, key: TraceKey) -> Arc<SessionFile> {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        let file = files.entry(key).or_insert_with_key(|key| {
            Arc::new(SessionFile {
                id: self.created.fetch_add(1, Ordering::Relaxed),
                label: key.label(),
            })
        });
        let file = Arc::clone(file);
        drop(files);
        file
    }

    /// The trace a request naming `session_id` belongs to: the session's own
    /// when the server assigned it this run, the `unassigned` trace otherwise.
    fn named(&self, session_id: &str) -> Arc<SessionFile> {
        let assigned = self
            .files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&TraceKey::Session(session_id.to_owned()))
            .map(Arc::clone);
        assigned.unwrap_or_else(|| self.file(TraceKey::Unassigned))
    }

    /// Reserves the next place in the `stateless` trace for a request naming
    /// no session (see [`Reservation`]).
    async fn reserve(&self) -> Reservation {
        let file = self.file(TraceKey::Stateless);
        let place = self.created.fetch_add(1, Ordering::Relaxed);
        let reserve = Command::Reserve {
            file: Arc::clone(&file),
            place,
        };
        // A reservation the writer never received is settled already: there
        // is nothing for its release to give up.
        let settled = self.sender.send(reserve).await.is_err();
        Reservation {
            sender: self.sender.clone(),
            file,
            place,
            settled,
        }
    }

    /// Enqueues one event for `file`; the writer task sequences it.
    /// On channel pressure this awaits (the exchange slows; the trace stays
    /// whole).
    async fn record(&self, file: &Arc<SessionFile>, direction: Direction, body: EventBody) {
        self.record_together(file, vec![(direction, body)]).await;
    }

    /// Enqueues `events` for `file` as one unit, written as adjacent lines in
    /// order (see [`Record`]).
    async fn record_together(&self, file: &Arc<SessionFile>, events: Vec<(Direction, EventBody)>) {
        let record = Record {
            file: Arc::clone(file),
            events,
        };
        if self.sender.send(Command::Write(record)).await.is_err() {
            eprintln!("mcp-everything-server: tap writer gone; event dropped");
        }
    }
}

mod files;
/// The writer task: per-file sequencing, reserved places, and naming.
mod writer;

#[cfg(test)]
use files::file_label;
use files::{Reservation, SessionFile, TraceKey};
use headers::{header_value, recorded_headers};
use writer::{Command, write_loop};

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
    let bytes = match buffer(body).await {
        Buffered::Whole(bytes) => bytes,
        Buffered::Passed(body) => {
            // Larger than the recording cap, or cut off: the service receives
            // every byte the client sent, and the tap records nothing of it.
            eprintln!(
                "mcp-everything-server: tap passed an oversized request body through unrecorded"
            );
            return next.run(Request::from_parts(parts, body)).await;
        }
    };
    let request_payload: Option<serde_json::Value> = serde_json::from_slice(&bytes).ok();
    if request_payload.is_none() && !bytes.is_empty() {
        // GET and DELETE bodies are legitimately empty; a non-empty body that
        // is not JSON cannot be recorded as a message. Say so rather than
        // leaving a silent hole a trace reader would misread as "no body".
        // tests/cli.rs counts these notes against the real binary's stderr:
        // exactly one for one bad body, none for clean traffic.
        eprintln!(
            "mcp-everything-server: tap recorded an exchange without its request \
             message (body is not JSON)"
        );
    }
    let request_events = request_events(&method, request_headers, request_payload);

    // Recorded now, as observed (see "`seq` is observation order"). A request
    // naming a session belongs to it, or to the `unassigned` trace when the
    // server never assigned it; one naming none waits for its response.
    let observed = match request_session {
        Some(session_id) => {
            let file = tap.named(&session_id);
            tap.record_together(&file, request_events).await;
            Ok(file)
        }
        None => Err((tap.reserve().await, request_events)),
    };
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;

    let file = match observed {
        Ok(file) => file,
        // The response names the session it assigned (on `initialize`); the
        // request is that session's first event, so nothing is out of order
        // when it moves there. Otherwise it fills its place in `stateless`.
        Err((reservation, request_events)) => {
            if let Some(assigned) = header_value(response.headers(), SESSION_ID_HEADER) {
                reservation.settle(Vec::new()).await;
                let file = tap.file(TraceKey::Session(assigned));
                tap.record_together(&file, request_events).await;
                file
            } else {
                let file = Arc::clone(&reservation.file);
                reservation.settle(request_events).await;
                file
            }
        }
    };

    record_response(&tap, &file, method, response).await
}

/// The client side of one exchange: its HTTP event and, when the body was
/// JSON, the message it carried.
fn request_events(
    method: &Method,
    headers: std::collections::BTreeMap<String, String>,
    payload: Option<serde_json::Value>,
) -> Vec<(Direction, EventBody)> {
    let mut request_events = vec![(
        Direction::ClientToServer,
        EventBody::Http {
            // Which request form this was. Streamable HTTP binds different
            // obligations to POST, GET, and DELETE, so a recording that drops
            // the method leaves the clauses addressed to one of them unjudgeable
            // — and, before 2026-08-20, made the validator fail a conforming
            // teardown DELETE with the POST clause's Accept requirement.
            method: Some(method.as_str().to_owned()),
            status: None,
            headers,
        },
    )];
    if let Some(payload) = payload {
        request_events.push((Direction::ClientToServer, EventBody::Message { payload }));
    }
    request_events
}

/// Records the response side of one exchange: the HTTP observation, the
/// transport-close moment on session DELETE, and the body's message(s) —
/// streamed frame-by-frame for SSE, buffered for JSON.
async fn record_response(
    tap: &Arc<Tap>,
    file: &Arc<SessionFile>,
    method: Method,
    response: Response,
) -> Response {
    let status = response.status();
    let content_type = header_value(response.headers(), "content-type");
    let status_event = (
        Direction::ServerToClient,
        EventBody::Http {
            // A response is not a request: the method belongs to the client
            // event recorded above, and repeating it here would invite a check
            // to judge a response against a request-side clause.
            method: None,
            status: Some(status.as_u16()),
            headers: recorded_headers(response.headers()),
        },
    );

    // Session teardown is a transport-close moment.
    if method == Method::DELETE && status.is_success() {
        let close = (
            Direction::ServerToClient,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose,
            },
        );
        tap.record_together(file, vec![status_event, close]).await;
        return response;
    }

    if content_type
        .as_deref()
        .is_some_and(|v| v.starts_with("text/event-stream"))
    {
        // A stream's frames arrive after its status by nature; each frame is
        // a message of its own, written as it flows.
        tap.record_together(file, vec![status_event]).await;
        return record_sse(tap, file, response);
    }
    if content_type
        .as_deref()
        .is_some_and(|v| v.starts_with("application/json"))
    {
        return record_json(tap, file, status_event, response).await;
    }
    tap.record_together(file, vec![status_event]).await;
    response
}

/// Re-bodies an SSE response with a recording pass-through: frames are
/// recorded as they flow, bytes are forwarded untouched.
fn record_sse(tap: &Arc<Tap>, file: &Arc<SessionFile>, response: Response) -> Response {
    let (parts, body) = response.into_parts();
    let tap = Arc::clone(tap);
    let file = Arc::clone(file);
    let mut splitter = SseSplitter::default();
    let stream = body.into_data_stream().then(move |chunk| {
        let tap = Arc::clone(&tap);
        let file = Arc::clone(&file);
        let payloads = chunk
            .as_ref()
            .map_or_else(|_| Vec::new(), |bytes| splitter.push(bytes));
        async move {
            for payload in payloads {
                tap.record(
                    &file,
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

/// Buffers, records, and re-bodies a JSON response, writing its status event
/// and its message as adjacent lines.
async fn record_json(
    tap: &Arc<Tap>,
    file: &Arc<SessionFile>,
    status_event: (Direction, EventBody),
    response: Response,
) -> Response {
    let (parts, body) = response.into_parts();
    match buffer(body).await {
        Buffered::Whole(bytes) => {
            let mut events = vec![status_event];
            if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                events.push((Direction::ServerToClient, EventBody::Message { payload }));
            }
            tap.record_together(file, events).await;
            Response::from_parts(parts, Body::from(bytes))
        }
        Buffered::Passed(body) => {
            tap.record_together(file, vec![status_event]).await;
            eprintln!(
                "mcp-everything-server: tap passed an oversized response body through unrecorded"
            );
            Response::from_parts(parts, body)
        }
    }
}

#[cfg(test)]
mod tests;
