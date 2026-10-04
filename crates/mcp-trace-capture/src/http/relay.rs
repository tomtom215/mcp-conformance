// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One request through the proxy: recorded, forwarded, and its response recorded
//! and relayed.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
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
    match proxy.client.request(upstream_request).await {
        Ok(upstream) => proxy.relay_response(upstream).await,
        Err(error) => proxy.upstream_failed(&error),
    }
}

impl Proxy {
    /// Records a request's `http` event and body, and returns the body to send
    /// upstream — `None` when the client's body could not be read.
    async fn record_request(&self, parts: &axum::http::request::Parts, body: Body) -> Option<Body> {
        // Metadata events cannot be refused for length (the line limit is at least
        // 1 MiB, and HTTP header blocks are bounded far below it), and a sink
        // failure is reported once, by `Recorder::finish`.
        let _ = self.recorder.record(
            Direction::ClientToServer,
            TransportKind::StreamableHttp,
            EventBody::Http {
                method: Some(parts.method.as_str().to_owned()),
                status: None,
                headers: headers::recorded(&parts.headers),
            },
        );
        let mut stream = body.into_data_stream();
        let (prefix, complete) = read_prefix(&mut stream, self.options.max_message)
            .await
            .ok()?;
        if complete {
            self.record_body(Direction::ClientToServer, &prefix);
            return Some(Body::from(prefix));
        }
        self.counters.oversized.fetch_add(1, Ordering::Relaxed);
        Some(Body::from_stream(
            futures::stream::once(async move { Ok::<_, axum::Error>(Bytes::from(prefix)) })
                .chain(stream),
        ))
    }

    /// Records the upstream's response and relays it to the client.
    async fn relay_response(
        self: &Arc<Self>,
        upstream: axum::http::Response<hyper::body::Incoming>,
    ) -> Response {
        let (parts, incoming) = upstream.into_parts();
        let _ = self.recorder.record(
            Direction::ServerToClient,
            TransportKind::StreamableHttp,
            EventBody::Http {
                method: None,
                status: Some(parts.status.as_u16()),
                headers: headers::recorded(&parts.headers),
            },
        );
        let mut stream = Body::new(incoming).into_data_stream();
        let body = if is_event_stream(&parts.headers) {
            Body::from_stream(Arc::clone(self).record_events(stream))
        } else {
            let Ok((prefix, complete)) = read_prefix(&mut stream, self.options.max_message).await
            else {
                return self.upstream_failed(&"the response body was cut off");
            };
            if complete {
                self.record_body(Direction::ServerToClient, &prefix);
                Body::from(prefix)
            } else {
                self.counters.oversized.fetch_add(1, Ordering::Relaxed);
                Body::from_stream(
                    futures::stream::once(async move { Ok::<_, axum::Error>(Bytes::from(prefix)) })
                        .chain(stream),
                )
            }
        };
        let mut response = Response::new(body);
        *response.status_mut() = parts.status;
        *response.headers_mut() = headers::forwardable(&parts.headers, &[header::CONTENT_LENGTH]);
        response
    }

    /// Counts a message the recorder refused for its length — one whose bytes fit
    /// the message limit but whose line, as written, would not.
    fn count_refused(&self, recorded: Result<u64, NotRecorded>) {
        if recorded == Err(NotRecorded::TooLong) {
            self.counters.oversized.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a complete body: nothing for an empty one, a message for JSON.
    fn record_body(&self, direction: Direction, body: &[u8]) {
        if body.is_empty() {
            return;
        }
        match parse_json(body) {
            Some(payload) => {
                self.count_refused(self.recorder.record(
                    direction,
                    TransportKind::StreamableHttp,
                    EventBody::Message { payload },
                ));
            }
            None => {
                self.counters.not_json.fetch_add(1, Ordering::Relaxed);
            }
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
        stream
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

    /// The upstream could not be reached or failed mid-response. The 502 is the
    /// proxy's, not the server's, so it is recorded as an aborted transport rather
    /// than as a response.
    fn upstream_failed(&self, error: &dyn std::fmt::Display) -> Response {
        eprintln!("mcp-trace-capture: upstream request failed: {error}");
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
        (
            StatusCode::BAD_GATEWAY,
            "mcp-trace-capture: upstream unreachable",
        )
            .into_response()
    }
}

/// Reads up to `max` bytes; `true` when that was the whole body.
pub(super) async fn read_prefix<S, E>(stream: &mut S, max: usize) -> Result<(Vec<u8>, bool), E>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    let mut prefix = Vec::new();
    while let Some(chunk) = stream.next().await {
        prefix.extend_from_slice(&chunk?);
        if prefix.len() > max {
            return Ok((prefix, false));
        }
    }
    Ok((prefix, true))
}
