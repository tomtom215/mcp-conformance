// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The answers the proxy gives on its own, with no response from the upstream
//! behind them: a request it refuses to forward, a client whose body stalled,
//! an upstream it could not reach or that cut its response off. Each is said
//! on stderr, counted for the summary at exit, and — where a request did reach
//! the trace — recorded as an aborted transport, never as a response the
//! server gave.

use std::sync::atomic::Ordering;

use axum::http::uri::{Authority, Uri};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};

use super::Proxy;
use crate::recorder::Recorder;

impl Proxy {
    /// A request whose path would leave the upstream's once the server resolves
    /// its `.` or `..` segments, or is not UTF-8 once decoded
    /// ([`is_refused_path`](super::target::is_refused_path)): answered 400 by
    /// the proxy, never forwarded, so not recorded — no server saw it.
    pub(super) fn refuse_path(&self) -> Response {
        let message = "mcp-trace-capture: the request path has a `.` or `..` segment (once \
                       percent-decoded, any `;parameters` aside), which would leave the \
                       upstream's path, or is not UTF-8 once decoded; refused, not forwarded";
        eprintln!("{message}");
        self.counters.refused_paths.fetch_add(1, Ordering::Relaxed);
        (StatusCode::BAD_REQUEST, message).into_response()
    }

    /// `CONNECT` asks a proxy for a tunnel to whatever host the request names;
    /// a recording tap in front of one server opens none: answered 405 by the
    /// proxy, never forwarded, so not recorded.
    pub(super) fn refuse_connect(&self) -> Response {
        let message = "mcp-trace-capture: CONNECT asks for a tunnel, which this proxy does not \
                       open; refused, not forwarded";
        eprintln!("{message}");
        self.counters
            .refused_connects
            .fetch_add(1, Ordering::Relaxed);
        (StatusCode::METHOD_NOT_ALLOWED, message).into_response()
    }

    /// No byte of the client's request body arrived within the body read
    /// timeout: answered 408, on a connection the proxy then closes, so a
    /// client that trickles bodies cannot hold connections and tasks open.
    /// The request's `http` event is in the trace already; the exchange is
    /// recorded as cut off, as a client gone mid-body is.
    pub(super) fn body_stalled(&self, recorder: &Recorder) -> Response {
        let message = format!(
            "mcp-trace-capture: no byte of a request's body arrived for {}s; answered 408 \
             and its connection closed",
            self.options.body_read_timeout.as_secs_f32()
        );
        eprintln!("{message}");
        self.counters.stalled_bodies.fetch_add(1, Ordering::Relaxed);
        let _ = recorder.record(
            Direction::ClientToServer,
            TransportKind::StreamableHttp,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportAbort,
            },
        );
        let mut response = (StatusCode::REQUEST_TIMEOUT, message).into_response();
        response.headers_mut().insert(
            header::CONNECTION,
            header::HeaderValue::from_static("close"),
        );
        response
    }

    /// The upstream failed mid-response, after its status and headers were
    /// relayed: the client sees the truncation; the trace records the transport as
    /// aborted.
    pub(super) fn upstream_cut(&self, recorder: &Recorder, error: &dyn std::error::Error) {
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
    pub(super) fn upstream_failed(
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
