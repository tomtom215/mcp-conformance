// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One request through the proxy: recorded, forwarded, and its response recorded
//! and relayed.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::uri::{Authority, Uri};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use futures::{Stream, StreamExt as _, TryStreamExt as _};
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};

use super::Proxy;
use super::target::{is_event_stream, target};
use crate::framing::{SseParser, parse_json};
use crate::headers;
use crate::recorder::NotRecorded;

pub(super) async fn forward(State(proxy): State<Arc<Proxy>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Some(upstream_body) = proxy.record_request(&parts, body).await else {
        // The client went away mid-body; there is nothing left to answer.
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut upstream_request = axum::http::Request::new(upstream_body);
    *upstream_request.method_mut() = parts.method;
    *upstream_request.uri_mut() = target(&proxy.options.upstream, parts.uri.path_and_query());
    let dropped: &[header::HeaderName] = if proxy.options.preserve_host {
        &[header::CONTENT_LENGTH, header::ACCEPT_ENCODING]
    } else {
        &[
            header::HOST,
            header::CONTENT_LENGTH,
            header::ACCEPT_ENCODING,
        ]
    };
    *upstream_request.headers_mut() = headers::forwardable(&parts.headers, dropped);
    let uri = upstream_request.uri().clone();
    match proxy.client.request(upstream_request).await {
        Ok(upstream) => proxy.relay_response(upstream).await,
        Err(error) => proxy.upstream_failed(&uri, &error),
    }
}

impl Proxy {
    /// Records a request's `http` event and body, and returns the body to send
    /// upstream — `None` when the client's body could not be read.
    ///
    /// The body is read (up to the message limit) before anything is recorded,
    /// and the `http` event and the message are then written together: the
    /// validator reads a message with the headers beside it, and a concurrent
    /// exchange recorded between the two would pair this body with its headers.
    async fn record_request(&self, parts: &axum::http::request::Parts, body: Body) -> Option<Body> {
        // Metadata events cannot be refused for length (the line limit is at least
        // 1 MiB, and HTTP header blocks are bounded far below it), and a sink
        // failure is reported once, by `Recorder::finish`.
        let http = EventBody::Http {
            method: Some(parts.method.as_str().to_owned()),
            status: None,
            headers: headers::recorded(&parts.headers),
        };
        let mut stream = body.into_data_stream();
        let prefix = read_prefix(&mut stream, self.options.max_message).await;
        let (message, forward) = match prefix.end {
            End::Complete => (
                self.message_of(&prefix.bytes),
                Some(Body::from(prefix.bytes)),
            ),
            End::Over => {
                self.counters.oversized.fetch_add(1, Ordering::Relaxed);
                (None, Some(then_rest(prefix.bytes, stream)))
            }
            End::Failed(_) => (None, None),
        };
        self.record_together(Direction::ClientToServer, http, message);
        forward
    }

    /// Records the upstream's response and relays it to the client.
    ///
    /// For a JSON body, the status event and the message are written together,
    /// for the reason [`Proxy::record_request`] gives. An event stream's status is
    /// recorded at once: its messages follow over time, each as it is relayed.
    async fn relay_response(
        self: &Arc<Self>,
        upstream: axum::http::Response<hyper::body::Incoming>,
    ) -> Response {
        let (parts, incoming) = upstream.into_parts();
        let http = EventBody::Http {
            method: None,
            status: Some(parts.status.as_u16()),
            headers: headers::recorded(&parts.headers),
        };
        let mut stream = Body::new(incoming).into_data_stream();
        let body = if is_event_stream(&parts.headers) {
            self.record_together(Direction::ServerToClient, http, None);
            Body::from_stream(Arc::clone(self).record_events(stream))
        } else {
            let prefix = read_prefix(&mut stream, self.options.max_message).await;
            match prefix.end {
                End::Complete => {
                    let message = self.message_of(&prefix.bytes);
                    self.record_together(Direction::ServerToClient, http, message);
                    Body::from(prefix.bytes)
                }
                End::Over => {
                    self.counters.oversized.fetch_add(1, Ordering::Relaxed);
                    self.record_together(Direction::ServerToClient, http, None);
                    then_rest(prefix.bytes, stream)
                }
                // The status and headers are the upstream's and are relayed as
                // they came; the body ends where the upstream's did, with an
                // error, so the client sees the same truncation it would have
                // seen directly — not a 502 the proxy made up.
                End::Failed(error) => {
                    self.record_together(Direction::ServerToClient, http, None);
                    self.upstream_cut(&error);
                    // Yield once first, so the head and the bytes before the cut
                    // are flushed to the client before the error aborts it.
                    then_rest(
                        prefix.bytes,
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
        *response.headers_mut() = headers::forwardable(&parts.headers, &[header::CONTENT_LENGTH]);
        response
    }

    /// Writes an `http` event and the message its body carried, if any, as
    /// adjacent lines.
    fn record_together(&self, direction: Direction, http: EventBody, message: Option<EventBody>) {
        let mut events = vec![(direction, TransportKind::StreamableHttp, http)];
        events.extend(message.map(|message| (direction, TransportKind::StreamableHttp, message)));
        let mut results = self.recorder.record_all(events).into_iter();
        // The http event cannot be refused for length (see `record_request`).
        let _ = results.next();
        if let Some(message) = results.next() {
            self.count_refused(message);
        }
    }

    /// The message a complete body carries: none for an empty one; for one that
    /// is not JSON, none, counted.
    fn message_of(&self, body: &[u8]) -> Option<EventBody> {
        if body.is_empty() {
            return None;
        }
        let payload = parse_json(body);
        if payload.is_none() {
            self.counters.not_json.fetch_add(1, Ordering::Relaxed);
        }
        payload.map(|payload| EventBody::Message { payload })
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
        stream: S,
    ) -> impl Stream<Item = Result<Bytes, axum::Error>>
    where
        S: Stream<Item = Result<Bytes, axum::Error>>,
    {
        let mut parser = SseParser::new(self.options.max_message);
        let mut oversized_seen = 0;
        // At shutdown the stream ends between two chunks, so the client sees a
        // stream that closed rather than a proxy that will not stop.
        let stopping = super::stopped(self.stopping.clone());
        // The upstream failing mid-stream reaches the client as the same
        // truncation, and the trace as an aborted transport.
        let on_error = Arc::clone(&self);
        stream
            .inspect_err(move |error| on_error.upstream_cut(error))
            .inspect_ok(move |chunk| {
                for data in parser.push(chunk) {
                    match parse_json(&data) {
                        Some(payload) => {
                            self.count_refused(self.recorder.record(
                                Direction::ServerToClient,
                                TransportKind::StreamableHttp,
                                EventBody::Message { payload },
                            ));
                        }
                        None => {
                            self.counters.not_json.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                // The parser's count only grows; add what this chunk contributed.
                let oversized = parser.oversized();
                self.counters
                    .oversized
                    .fetch_add(oversized - oversized_seen, Ordering::Relaxed);
                oversized_seen = oversized;
            })
            .take_until(stopping)
    }

    /// The upstream failed mid-response, after its status and headers were
    /// relayed: the client sees the truncation; the trace records the transport as
    /// aborted.
    fn upstream_cut(&self, error: &dyn std::error::Error) {
        eprintln!(
            "mcp-trace-capture: the upstream's response was cut off ({}); the truncation \
             is relayed to the client",
            causes(error)
        );
        self.counters.upstream_cut.fetch_add(1, Ordering::Relaxed);
        let _ = self.recorder.record(
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
    fn upstream_failed(&self, uri: &Uri, error: &dyn std::error::Error) -> Response {
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
        let _ = self.recorder.record(
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

/// A body of `prefix` followed by whatever `rest` yields.
fn then_rest<S>(prefix: Vec<u8>, rest: S) -> Body
where
    S: Stream<Item = Result<Bytes, axum::Error>> + Send + 'static,
{
    Body::from_stream(
        futures::stream::once(async move { Ok::<_, axum::Error>(Bytes::from(prefix)) }).chain(rest),
    )
}

/// How a body read by [`read_prefix`] stopped.
#[derive(Debug)]
pub(super) enum End<E> {
    /// The body ended within the limit: the prefix is all of it.
    Complete,
    /// The body passed the limit; the rest is still in the stream.
    Over,
    /// The body failed before either.
    Failed(E),
}

/// The start of a body, and how reading it stopped.
#[derive(Debug)]
pub(super) struct Prefix<E> {
    pub(super) bytes: Vec<u8>,
    pub(super) end: End<E>,
}

/// Reads until the body ends, fails, or passes `max` bytes — keeping what was
/// read in every case.
pub(super) async fn read_prefix<S, E>(stream: &mut S, max: usize) -> Prefix<E>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(error) => {
                return Prefix {
                    bytes,
                    end: End::Failed(error),
                };
            }
        }
        if bytes.len() > max {
            return Prefix {
                bytes,
                end: End::Over,
            };
        }
    }
    Prefix {
        bytes,
        end: End::Complete,
    }
}
