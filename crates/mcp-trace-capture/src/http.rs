// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The HTTP recorder: a reverse proxy in front of a streamable-HTTP MCP server.
//!
//! Point a client at the proxy instead of the server. Every request is forwarded to
//! `upstream` with the same method, path, query, headers and body; every response
//! comes back the same way, SSE streams included, event by event as they arrive. The
//! trace gets an `http` event per request and per response (the allowlisted headers,
//! the method or status) and a `message` event per JSON-RPC payload.
//!
//! **What changes on the wire, and why.** Hop-by-hop headers (RFC 9110 §7.6.1) are the
//! proxy's by definition. `Host` names the upstream, since that is where the request
//! now goes — unless [`Options::preserve_host`] is set, in which case the server sees
//! the `Host` the client sent.
//! `Accept-Encoding` is removed from requests, so the server answers
//! uncompressed and the recording can read the body; everything else, `Origin` and
//! credentials included, passes through as sent.
//!
//! **Ordering.** A request's events are recorded before it is sent upstream, and a
//! response's before it is returned — for SSE, each event before the bytes carrying it
//! are passed on — so `seq` order is causal.
//!
//! **Limits.** A body or SSE event larger than the message limit is forwarded intact
//! and counted, not recorded: the proxy never alters traffic to fit its trace.

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::uri::{PathAndQuery, Uri};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use futures::{Stream, StreamExt as _, TryStreamExt as _};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};

use crate::framing::{SseParser, parse_json};
use crate::headers;
use crate::recorder::Recorder;

#[cfg(feature = "tls")]
type Connector = hyper_rustls::HttpsConnector<HttpConnector>;
#[cfg(not(feature = "tls"))]
type Connector = HttpConnector;

/// Where to forward, and how much of any one message to keep.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    /// The server's base URL. Request paths are appended to its path, so
    /// `http://localhost:3000` forwards `/mcp` to `http://localhost:3000/mcp`.
    pub upstream: Uri,
    /// The largest body or SSE event recorded; larger ones are forwarded unrecorded.
    pub max_message: usize,
    /// Forward the client's `Host` header instead of naming the upstream, so a
    /// server's own `Host` validation judges what the client sent rather than what
    /// the proxy sent. Wrong for a virtually hosted remote server, which routes on
    /// `Host`.
    pub preserve_host: bool,
}

impl Options {
    /// Options forwarding to `upstream`, keeping messages up to `max_message` bytes.
    ///
    /// # Errors
    ///
    /// `upstream` is not an absolute `http` URL, or an `https` one in a build
    /// without the `tls` feature.
    pub fn new(upstream: Uri, max_message: usize) -> Result<Self, String> {
        match (upstream.scheme_str(), upstream.authority()) {
            (Some("http"), Some(_)) => {}
            (Some("https"), Some(_)) if cfg!(feature = "tls") => {}
            (Some("https"), Some(_)) => {
                return Err("this build has no TLS support (feature `tls`)".to_owned());
            }
            _ => return Err(format!("{upstream} is not an absolute http or https URL")),
        }
        Ok(Self {
            upstream,
            max_message,
            preserve_host: false,
        })
    }
}

/// What the proxy forwarded without recording.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unrecorded {
    /// Bodies or SSE events that were not JSON.
    pub not_json: u64,
    /// Bodies or SSE events over the message limit.
    pub oversized: u64,
    /// Requests the upstream could not be reached for (answered 502 by the proxy).
    pub upstream_failures: u64,
}

#[derive(Debug, Default)]
struct Counters {
    not_json: AtomicU64,
    oversized: AtomicU64,
    upstream_failures: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> Unrecorded {
        Unrecorded {
            not_json: self.not_json.load(Ordering::Relaxed),
            oversized: self.oversized.load(Ordering::Relaxed),
            upstream_failures: self.upstream_failures.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug)]
struct Proxy {
    recorder: Arc<Recorder>,
    client: Client<Connector, Body>,
    options: Options,
    counters: Counters,
}

/// Serves the proxy on `listener` until `shutdown` resolves.
///
/// # Errors
///
/// The platform's root certificates could not be loaded (`tls` builds), or the
/// server failed.
pub async fn serve(
    listener: tokio::net::TcpListener,
    recorder: Arc<Recorder>,
    options: Options,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<Unrecorded> {
    let proxy = Arc::new(Proxy {
        recorder,
        client: Client::builder(TokioExecutor::new()).build(connector()?),
        options,
        counters: Counters::default(),
    });
    let app = axum::Router::new()
        .fallback(forward)
        .with_state(Arc::clone(&proxy));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(proxy.counters.snapshot())
}

/// The upstream connector: HTTPS and HTTP with the `tls` feature, HTTP without.
///
/// One function with the variants inside, rather than one function per `cfg`, so
/// that every build compiles the whole of it and mutation testing (which runs
/// with all features) never mutates a body that build does not contain.
#[cfg_attr(
    not(feature = "tls"),
    allow(
        clippy::unnecessary_wraps,
        reason = "only loading root certificates can fail"
    )
)]
fn connector() -> io::Result<Connector> {
    #[cfg(feature = "tls")]
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_provider_and_native_roots(rustls::crypto::ring::default_provider())?
        .https_or_http()
        .enable_http1()
        .build();
    #[cfg(not(feature = "tls"))]
    let connector = HttpConnector::new();
    Ok(connector)
}

async fn forward(State(proxy): State<Arc<Proxy>>, request: Request) -> Response {
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
        self.recorder.record(
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
        self.recorder.record(
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

    /// Records a complete body: nothing for an empty one, a message for JSON.
    fn record_body(&self, direction: Direction, body: &[u8]) {
        if body.is_empty() {
            return;
        }
        match parse_json(body) {
            Some(payload) => {
                self.recorder.record(
                    direction,
                    TransportKind::StreamableHttp,
                    EventBody::Message { payload },
                );
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
        stream.inspect_ok(move |chunk| {
            for data in parser.push(chunk) {
                match parse_json(&data) {
                    Some(payload) => {
                        self.recorder.record(
                            Direction::ServerToClient,
                            TransportKind::StreamableHttp,
                            EventBody::Message { payload },
                        );
                    }
                    None => {
                        self.counters.not_json.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let oversized = parser.oversized();
            if oversized > oversized_seen {
                self.counters
                    .oversized
                    .fetch_add(oversized - oversized_seen, Ordering::Relaxed);
                oversized_seen = oversized;
            }
        })
    }

    /// The upstream could not be reached or failed mid-response. The 502 is the
    /// proxy's, not the server's, so it is recorded as an aborted transport rather
    /// than as a response.
    fn upstream_failed(&self, error: &dyn std::fmt::Display) -> Response {
        eprintln!("mcp-trace-capture: upstream request failed: {error}");
        self.counters
            .upstream_failures
            .fetch_add(1, Ordering::Relaxed);
        self.recorder.record(
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
async fn read_prefix<S, E>(stream: &mut S, max: usize) -> Result<(Vec<u8>, bool), E>
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

/// `upstream` with the request's path appended to its own, and the request's query.
fn target(upstream: &Uri, path_and_query: Option<&PathAndQuery>) -> Uri {
    let base = upstream.path().trim_end_matches('/');
    let path = path_and_query.map_or("/", PathAndQuery::path);
    let joined = path_and_query.and_then(PathAndQuery::query).map_or_else(
        || format!("{base}{path}"),
        |query| format!("{base}{path}?{query}"),
    );
    let mut parts = upstream.clone().into_parts();
    // `joined` is a path the client sent plus a prefix `Uri` already accepted, so it
    // is a valid path-and-query; fall back to the upstream's own if not.
    parts.path_and_query = joined.parse().ok().or(parts.path_and_query);
    Uri::from_parts(parts).unwrap_or_else(|_| upstream.clone())
}

fn is_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn target_appends_the_request_path_to_the_upstream_path() {
        let pq = |text: &str| text.parse::<PathAndQuery>().unwrap();
        let root: Uri = "http://localhost:3000".parse().unwrap();
        assert_eq!(
            target(&root, Some(&pq("/mcp?x=1"))).to_string(),
            "http://localhost:3000/mcp?x=1"
        );
        let prefixed: Uri = "https://example.com/api/".parse().unwrap();
        assert_eq!(
            target(&prefixed, Some(&pq("/mcp"))).to_string(),
            "https://example.com/api/mcp"
        );
        assert_eq!(target(&root, None).to_string(), "http://localhost:3000/");
    }

    #[test]
    fn only_absolute_http_upstreams_are_accepted() {
        let uri = |text: &str| text.parse::<Uri>().unwrap();
        assert!(Options::new(uri("http://127.0.0.1:3000"), 1).is_ok());
        assert_eq!(
            Options::new(uri("https://example.com"), 1).is_ok(),
            cfg!(feature = "tls")
        );
        assert!(Options::new(uri("/relative"), 1).is_err());
        assert!(Options::new(uri("ftp://example.com"), 1).is_err());
    }

    #[test]
    fn event_streams_are_recognised_with_parameters_and_any_case() {
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, value.parse().unwrap());
            headers
        };
        assert!(is_event_stream(&with("text/event-stream")));
        assert!(is_event_stream(&with("Text/Event-Stream; charset=utf-8")));
        assert!(!is_event_stream(&with("application/json")));
        assert!(!is_event_stream(&HeaderMap::new()));
    }
}
