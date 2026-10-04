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

use std::future::{Future, IntoFuture as _};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::uri::Uri;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use mcp_conformance_core::trace::{Direction, LifecycleEvent, TransportKind};
use tokio::sync::watch;

use crate::recorder::Recorder;

mod relay;
mod target;

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
    /// How long open connections get to finish once [`serve`]'s `shutdown`
    /// resolves, before the proxy stops without them. Event streams do not wait
    /// for it: they are ended at once (an MCP client holds a GET stream open for
    /// its whole session, so waiting for one to finish would never stop).
    pub grace: std::time::Duration,
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
            grace: crate::STOP_GRACE,
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
    /// Responses the upstream cut off after its status and headers (relayed to
    /// the client as the same truncation; recorded as `transport-abort`).
    pub upstream_cut: u64,
}

#[derive(Debug, Default)]
struct Counters {
    not_json: AtomicU64,
    oversized: AtomicU64,
    upstream_failures: AtomicU64,
    upstream_cut: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> Unrecorded {
        Unrecorded {
            not_json: self.not_json.load(Ordering::Relaxed),
            oversized: self.oversized.load(Ordering::Relaxed),
            upstream_failures: self.upstream_failures.load(Ordering::Relaxed),
            upstream_cut: self.upstream_cut.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug)]
struct Proxy {
    recorder: Arc<Recorder>,
    client: Client<Connector, Body>,
    options: Options,
    counters: Counters,
    /// Becomes `true` when the proxy is shutting down.
    stopping: watch::Receiver<bool>,
}

/// Serves the proxy on `listener` until `shutdown` resolves, then stops.
///
/// Stopping ends every open event stream (cleanly, between two chunks the
/// upstream sent), gives other connections [`Options::grace`] to finish, and
/// closes the trace with a `transport-close` event — so a client holding a stream
/// open cannot keep the proxy running.
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
    let (stop, stopping) = watch::channel(false);
    let grace = options.grace;
    let proxy = Arc::new(Proxy {
        recorder,
        client: Client::builder(TokioExecutor::new()).build(connector()?),
        options,
        counters: Counters::default(),
        stopping: stopping.clone(),
    });
    let app = axum::Router::new()
        .fallback(relay::forward)
        .with_state(Arc::clone(&proxy));
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.await;
            let _ = stop.send(true);
        })
        .into_future();
    tokio::pin!(server);
    let waited_out = async {
        stopped(stopping).await;
        tokio::time::sleep(grace).await;
    };
    tokio::select! {
        result = &mut server => result?,
        () = waited_out => {
            eprintln!(
                "mcp-trace-capture: connections still open {}s after the stop request are \
                 closed unfinished",
                grace.as_secs_f32()
            );
        }
    }
    let _ = proxy.recorder.close(
        Direction::ClientToServer,
        TransportKind::StreamableHttp,
        LifecycleEvent::TransportClose,
    );
    Ok(proxy.counters.snapshot())
}

/// Resolves once `stopping` is `true`; never, if its sender went away without
/// setting it.
async fn stopped(mut stopping: watch::Receiver<bool>) {
    if stopping.wait_for(|stopping| *stopping).await.is_err() {
        std::future::pending::<()>().await;
    }
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

#[cfg(test)]
mod tests;
