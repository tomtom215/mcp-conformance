// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One request through the proxy: recorded, forwarded, and its response recorded
//! and relayed.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::uri::{Authority, Uri};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use futures::{Stream, TryStreamExt as _};
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};

use super::Proxy;
use super::body::{self, End, Held, cut_at_stop, read_prefix, then_rest};
use super::target::{has_dot_segment, is_event_stream, location, target};
use crate::framing::{Parsed, SseParser, parse_message};
use crate::headers;
use crate::recorder::{NotRecorded, Recorder};
use crate::traces::Route;

/// Records `request`, forwards it upstream, and records and relays the
/// response; `local` is the proxy's address the client's connection reached.
pub(super) async fn forward(
    proxy: Arc<Proxy>,
    local: Option<std::net::SocketAddr>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    if has_dot_segment(parts.uri.path()) {
        return proxy.refuse_path();
    }
    // An event stream answering a GET is the standalone stream a client holds
    // open for its whole session; one answering a POST carries that request's
    // response.
    let standalone = parts.method == axum::http::Method::GET;
    // A `DELETE` naming a session ends it, once the server agrees.
    let ending = (parts.method == axum::http::Method::DELETE)
        .then(|| parts.headers.get("mcp-session-id")?.to_str().ok())
        .flatten()
        .map(str::to_owned);
    let (route, upstream_body) = proxy.record_request(&parts, body).await;
    let Some(upstream_body) = upstream_body else {
        // The client went away mid-body; there is nothing left to answer.
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut upstream_request = axum::http::Request::new(upstream_body);
    *upstream_request.method_mut() = parts.method;
    *upstream_request.uri_mut() = target(&proxy.options.upstream, parts.uri.path_and_query());
    // `Expect: 100-continue` was the proxy's to answer, and it has: hyper sent
    // the `100 Continue` when the body was read. `Content-Length` stays: it
    // frames a body read whole (which has exactly that length) and one
    // streamed past the message limit alike.
    let dropped: &[header::HeaderName] = if proxy.options.preserve_host {
        &[header::ACCEPT_ENCODING, header::EXPECT]
    } else {
        &[header::HOST, header::ACCEPT_ENCODING, header::EXPECT]
    };
    *upstream_request.headers_mut() = headers::forwardable(&parts.headers, dropped);
    let uri = upstream_request.uri().clone();
    // Not the client's `Host`: any client can send any name there, and a
    // redirect would then send the next client to it.
    let host = local.map(|local| local.to_string());
    match proxy.client.request(upstream_request).await {
        Ok(upstream) => {
            if let Some(id) = ending.filter(|_| upstream.status().is_success()) {
                proxy.traces.end(&id);
            }
            proxy
                .relay_response(upstream, route, host.as_deref(), standalone)
                .await
        }
        Err(error) => proxy.upstream_failed(route.recorder(), &uri, &error),
    }
}

impl Proxy {
    /// Records a request's `http` event and body in its session's trace, and
    /// returns that trace's route and the body to send upstream — `None` when the
    /// client's body could not be read, which is recorded as `transport-abort`.
    ///
    /// The body is read (up to the message limit) before anything is recorded,
    /// and the `http` event and the message are then written together: the
    /// validator reads a message with the headers beside it, and a concurrent
    /// exchange recorded between the two would pair this body with its headers.
    async fn record_request(
        &self,
        parts: &axum::http::request::Parts,
        body: Body,
    ) -> (Route, Option<Body>) {
        // Metadata events cannot be refused for length (the line limit is at least
        // 1 MiB, and HTTP header blocks are bounded far below it), and a sink
        // failure is reported once, by `Recorder::finish`.
        let http = EventBody::Http {
            method: Some(parts.method.as_str().to_owned()),
            status: None,
            headers: headers::recorded(&parts.headers),
        };
        let mut stream = body.into_data_stream();
        let prefix = read_prefix(&mut stream, self.options.max_message, &self.budget).await;
        let session_id = parts
            .headers
            .get("mcp-session-id")
            .and_then(|id| id.to_str().ok());
        let whole = matches!(prefix.end, End::Complete).then_some(prefix.bytes.as_slice());
        let route = self.traces.route(session_id, whole);
        let (message, forward) = match prefix.end {
            End::Complete => (
                self.message_of(&prefix.bytes),
                Some(body::whole(prefix.bytes, prefix.held)),
            ),
            End::Over | End::OverBudget => {
                self.count_unread(&prefix.end);
                (None, Some(then_rest(prefix.bytes, prefix.held, stream)))
            }
            End::Failed(_) => (None, None),
        };
        self.record_together(route.recorder(), Direction::ClientToServer, http, message);
        if forward.is_none() {
            // The client's body failed (it went away mid-body): the request is
            // recorded but never sent, and the trace says the exchange was cut
            // off, as it does when the upstream's side fails.
            let _ = route.recorder().record(
                Direction::ClientToServer,
                TransportKind::StreamableHttp,
                EventBody::Lifecycle {
                    event: LifecycleEvent::TransportAbort,
                },
            );
        }
        (route, forward)
    }

    /// Records the upstream's response and relays it to the client.
    ///
    /// For a JSON body, the status event and the message are written together,
    /// for the reason [`Proxy::record_request`] gives. An event stream's status is
    /// recorded at once: its messages follow over time, each as it is relayed. A
    /// `standalone` one (the answer to a GET) is cut when the proxy stops; one
    /// answering a POST carries a request's response, and gets the grace period
    /// every other request in flight gets.
    ///
    /// A session id the response assigns is noted before the response is
    /// returned, so the client's next request — which names it — is routed to
    /// this session's trace. A redirect into the upstream is pointed back at
    /// the proxy, at `host`: the address the client's connection reached.
    async fn relay_response(
        self: &Arc<Self>,
        upstream: axum::http::Response<hyper::body::Incoming>,
        route: Route,
        host: Option<&str>,
        standalone: bool,
    ) -> Response {
        let (parts, incoming) = upstream.into_parts();
        self.count_status(parts.status);
        self.traces.respond(
            &route,
            parts
                .headers
                .get("mcp-session-id")
                .and_then(|id| id.to_str().ok()),
            parts.status.is_success(),
        );
        let recorder = route.recorder();
        let http = EventBody::Http {
            method: None,
            status: Some(parts.status.as_u16()),
            headers: headers::recorded(&parts.headers),
        };
        let mut stream = Body::new(incoming).into_data_stream();
        let body = if is_event_stream(&parts.headers) {
            self.record_together(recorder, Direction::ServerToClient, http, None);
            let events = Arc::clone(self).record_events(route.clone(), stream);
            if standalone {
                Body::from_stream(cut_at_stop(events, super::stopped(self.stopping.clone())))
            } else {
                Body::from_stream(events)
            }
        } else {
            let prefix = read_prefix(&mut stream, self.options.max_message, &self.budget).await;
            match prefix.end {
                End::Complete => {
                    let message = self.message_of(&prefix.bytes);
                    self.record_together(recorder, Direction::ServerToClient, http, message);
                    body::whole(prefix.bytes, prefix.held)
                }
                End::Over | End::OverBudget => {
                    self.count_unread(&prefix.end);
                    self.record_together(recorder, Direction::ServerToClient, http, None);
                    then_rest(prefix.bytes, prefix.held, stream)
                }
                // The status and headers are the upstream's and are relayed as
                // they came; the body ends where the upstream's did, with an
                // error, so the client sees the same truncation it would have
                // seen directly — not a 502 the proxy made up.
                End::Failed(error) => {
                    self.record_together(recorder, Direction::ServerToClient, http, None);
                    self.upstream_cut(recorder, &error);
                    // Yield once first, so the head and the bytes before the cut
                    // are flushed to the client before the error aborts it.
                    then_rest(
                        prefix.bytes,
                        prefix.held,
                        futures::stream::once(async move {
                            tokio::task::yield_now().await;
                            Err(error)
                        }),
                    )
                }
            }
        };
        let mut response = Response::new(body);
        *response.status_mut() = parts.status;
        *response.headers_mut() = self.response_headers(&parts.headers, host);
        response
    }

    /// The headers relayed with a response: the upstream's, less hop-by-hop
    /// fields, with a `Location` into the upstream pointed back at the proxy.
    /// `Content-Length` is kept — for a `HEAD` response it is the only length
    /// there is, and for a body streamed past the limit it keeps its framing.
    fn response_headers(&self, sent: &header::HeaderMap, host: Option<&str>) -> header::HeaderMap {
        let mut relayed = headers::forwardable(sent, &[]);
        let back = sent
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| location(&self.options.upstream, value, host))
            .and_then(|back| header::HeaderValue::from_str(&back).ok());
        if let Some(back) = back {
            relayed.insert(header::LOCATION, back);
        }
        relayed
    }

    /// Counts a status outside 2xx, and a 404 apart: the summary asks about
    /// the endpoint path when there were any.
    fn count_status(&self, status: StatusCode) {
        if !status.is_success() {
            self.counters.not_2xx.fetch_add(1, Ordering::Relaxed);
        }
        if status == StatusCode::NOT_FOUND {
            self.counters.not_found.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Writes an `http` event and the message its body carried, if any, as
    /// adjacent lines.
    fn record_together(
        &self,
        recorder: &Recorder,
        direction: Direction,
        http: EventBody,
        message: Option<EventBody>,
    ) {
        let mut events = vec![(direction, TransportKind::StreamableHttp, http)];
        events.extend(message.map(|message| (direction, TransportKind::StreamableHttp, message)));
        let mut results = recorder.record_all(events).into_iter();
        // The http event cannot be refused for length (see `record_request`).
        let _ = results.next();
        if let Some(message) = results.next() {
            self.count_refused(message);
        }
    }

    /// The message a complete body carries: none for an empty one; for one that
    /// is not JSON, or JSON the trace cannot hold as sent, none, counted.
    fn message_of(&self, body: &[u8]) -> Option<EventBody> {
        if body.is_empty() {
            return None;
        }
        let count = match parse_message(body) {
            Parsed::Json(payload) => return Some(EventBody::Message { payload }),
            Parsed::Altered => &self.counters.altered,
            Parsed::NotJson => &self.counters.not_json,
        };
        count.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Counts a body streamed on unrecorded: over the message limit, or past
    /// what was left of the buffer budget.
    fn count_unread<E>(&self, end: &End<E>) {
        let count = if matches!(end, End::OverBudget) {
            &self.counters.over_budget
        } else {
            &self.counters.oversized
        };
        count.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a message the recorder refused for its length — one whose bytes fit
    /// the message limit but whose line, as written, would not.
    fn count_refused(&self, recorded: Result<u64, NotRecorded>) {
        if recorded == Err(NotRecorded::TooLong) {
            self.counters.oversized.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Passes an SSE stream through, recording each event's JSON before the chunk
    /// that completes it is yielded.
    fn record_events<S>(
        self: Arc<Self>,
        route: Route,
        stream: S,
    ) -> impl Stream<Item = Result<Bytes, axum::Error>>
    where
        S: Stream<Item = Result<Bytes, axum::Error>>,
    {
        let mut parser = SseParser::new(self.options.max_message);
        let (mut oversized_seen, mut other_seen) = (0, 0);
        // What the parser holds of the event it is assembling is held in the
        // buffer budget too; an event that would take more than is left is
        // dropped (relayed, not recorded) and counted.
        let mut held = Held::default();
        // The upstream failing mid-stream reaches the client as the same
        // truncation, and the trace as an aborted transport.
        let on_error = Arc::clone(&self);
        let error_route = route.clone();
        stream
            .inspect_err(move |error| on_error.upstream_cut(error_route.recorder(), error))
            .inspect_ok(move |chunk| {
                for data in parser.push(chunk) {
                    if let Some(message) = self.message_of(&data) {
                        self.count_refused(route.recorder().record(
                            Direction::ServerToClient,
                            TransportKind::StreamableHttp,
                            message,
                        ));
                    }
                }
                if !held.resize(&self.budget, parser.buffered()) {
                    parser.drop_event();
                    self.counters.over_budget.fetch_add(1, Ordering::Relaxed);
                    held.resize(&self.budget, parser.buffered());
                }
                // The parser's counts only grow; add what this chunk contributed.
                let (oversized, other) = (parser.oversized(), parser.other_types());
                self.counters
                    .oversized
                    .fetch_add(oversized - oversized_seen, Ordering::Relaxed);
                self.counters
                    .other_events
                    .fetch_add(other - other_seen, Ordering::Relaxed);
                (oversized_seen, other_seen) = (oversized, other);
            })
    }

    /// A request whose path would leave the upstream's once the server resolves
    /// its `.` or `..` segments: answered 400 by the proxy, never forwarded, so
    /// not recorded — no server saw it.
    fn refuse_path(&self) -> Response {
        let message = "mcp-trace-capture: the request path has a `.` or `..` segment, which \
                       would leave the upstream's path; refused, not forwarded";
        eprintln!("{message}");
        self.counters.refused_paths.fetch_add(1, Ordering::Relaxed);
        (StatusCode::BAD_REQUEST, message).into_response()
    }

    /// The upstream failed mid-response, after its status and headers were
    /// relayed: the client sees the truncation; the trace records the transport as
    /// aborted.
    fn upstream_cut(&self, recorder: &Recorder, error: &dyn std::error::Error) {
        eprintln!(
            "mcp-trace-capture: the upstream's response was cut off ({}); the truncation \
             is relayed to the client",
            causes(error)
        );
        self.counters.upstream_cut.fetch_add(1, Ordering::Relaxed);
        let _ = recorder.record(
            Direction::ServerToClient,
            TransportKind::StreamableHttp,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportAbort,
            },
        );
    }

    /// The upstream could not be reached, so there is no response to relay. The
    /// 502 is the proxy's, not the server's, so it is recorded as an aborted
    /// transport rather than as a response; its body names the upstream (without
    /// the query, which can carry credentials) and the cause.
    fn upstream_failed(
        &self,
        recorder: &Recorder,
        uri: &Uri,
        error: &dyn std::error::Error,
    ) -> Response {
        let upstream = format!(
            "{}://{}{}",
            uri.scheme_str().unwrap_or("http"),
            uri.authority().map_or("", Authority::as_str),
            uri.path()
        );
        let message = format!(
            "mcp-trace-capture: cannot reach the upstream {upstream}: {}",
            causes(error)
        );
        eprintln!("{message}");
        self.counters
            .upstream_failures
            .fetch_add(1, Ordering::Relaxed);
        let _ = recorder.record(
            Direction::ServerToClient,
            TransportKind::StreamableHttp,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportAbort,
            },
        );
        (StatusCode::BAD_GATEWAY, message).into_response()
    }
}

/// `error` and the errors behind it, each once: an HTTP client's own message
/// ("client error (Connect)") says little without its sources.
fn causes(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}
