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
//! and counted, not recorded: the proxy never alters traffic to fit its trace. So
//! is one that is not JSON, or is JSON the trace cannot hold as sent, and an SSE
//! event of a type other than `message`.

use std::future::{Future, IntoFuture as _};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::uri::Uri;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use mcp_conformance_core::trace::LifecycleEvent;
use tokio::sync::watch;

use crate::recorder::Recorder;
use crate::traces::Traces;

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
    /// `http://localhost:3000` forwards `/mcp` to `http://localhost:3000/mcp`,
    /// and the root path is the URL itself: with `http://localhost:3000/mcp`,
    /// `/` is forwarded to `/mcp` and `/x` to `/mcp/x`. Its query, if any, is
    /// kept, and the request's appended to it.
    ///
    /// The connection settings are made for it by [`Options::new`]: change it
    /// from `http` to `https` afterwards and no certificate is trusted.
    pub upstream: Uri,
    /// The largest body or SSE event recorded; larger ones are forwarded unrecorded.
    pub max_message: usize,
    /// Forward the client's `Host` header instead of naming the upstream, so a
    /// server's own `Host` validation judges what the client sent rather than what
    /// the proxy sent. Wrong for a virtually hosted remote server, which routes on
    /// `Host`.
    pub preserve_host: bool,
    /// How long open connections get to finish once [`serve`]'s `shutdown`
    /// resolves, before the proxy stops without them. A standalone event stream
    /// (the answer to a GET) does not wait for it: it is cut at once (an MCP
    /// client holds one open for its whole session, so waiting for it to finish
    /// would never stop). An event stream answering a POST does: it carries a
    /// request's response.
    pub grace: std::time::Duration,
    /// How the proxy connects upstream, made once the upstream is known.
    connector: Connector,
}

impl Options {
    /// Options forwarding to `upstream`, keeping messages up to `max_message` bytes.
    ///
    /// # Errors
    ///
    /// `upstream` is not an absolute `http` URL, or an `https` one in a build
    /// without the `tls` feature, or it carries credentials (`user:password@`);
    /// or, for an `https` one, the platform's root certificates could not be
    /// loaded. Checked here, so a caller can refuse to start before it has
    /// created anything (a trace file).
    pub fn new(upstream: Uri, max_message: usize) -> Result<Self, String> {
        // Checked first, and the URL not repeated: the proxy would never send these
        // credentials, and echoing them would put a password on the terminal.
        if upstream
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
        {
            return Err(
                "the URL carries credentials (user:password@), which the proxy would not \
                 send; have the client send them instead, e.g. as an Authorization header"
                    .to_owned(),
            );
        }
        match (upstream.scheme_str(), upstream.authority()) {
            (Some("http"), Some(_)) => {}
            (Some("https"), Some(_)) if cfg!(feature = "tls") => {}
            (Some("https"), Some(_)) => {
                return Err("this build has no TLS support (feature `tls`)".to_owned());
            }
            // Not repeated either: a query can carry a key.
            _ => return Err("not an absolute http:// or https:// URL".to_owned()),
        }
        let connector = connector(upstream.scheme_str() == Some("https")).map_err(|error| {
            format!("cannot load the platform's root certificates for an https upstream: {error}")
        })?;
        Ok(Self {
            upstream,
            max_message,
            preserve_host: false,
            grace: crate::STOP_GRACE,
            connector,
        })
    }
}

/// What the proxy forwarded without recording.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unrecorded {
    /// Bodies or SSE events that were not JSON.
    pub not_json: u64,
    /// Bodies or SSE events that were JSON the trace cannot hold as sent
    /// ([`Parsed::Altered`](crate::framing::Parsed::Altered)).
    pub altered: u64,
    /// SSE events of a type other than `message`, which MCP clients ignore.
    pub other_events: u64,
    /// Bodies or SSE events over the message limit.
    pub oversized: u64,
    /// Requests the upstream could not be reached for (answered 502 by the proxy).
    pub upstream_failures: u64,
    /// Responses the upstream cut off after its status and headers (relayed to
    /// the client as the same truncation; recorded as `transport-abort`).
    pub upstream_cut: u64,
    /// Responses whose status was not 2xx (recorded as the server sent them).
    pub not_2xx: u64,
    /// Of those, `404 Not Found`: usually a client pointed at the wrong path.
    pub not_found: u64,
}

#[derive(Debug, Default)]
struct Counters {
    not_json: AtomicU64,
    altered: AtomicU64,
    other_events: AtomicU64,
    oversized: AtomicU64,
    upstream_failures: AtomicU64,
    upstream_cut: AtomicU64,
    not_2xx: AtomicU64,
    not_found: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> Unrecorded {
        Unrecorded {
            not_json: self.not_json.load(Ordering::Relaxed),
            altered: self.altered.load(Ordering::Relaxed),
            other_events: self.other_events.load(Ordering::Relaxed),
            oversized: self.oversized.load(Ordering::Relaxed),
            upstream_failures: self.upstream_failures.load(Ordering::Relaxed),
            upstream_cut: self.upstream_cut.load(Ordering::Relaxed),
            not_2xx: self.not_2xx.load(Ordering::Relaxed),
            not_found: self.not_found.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug)]
struct Proxy {
    traces: Arc<Traces>,
    client: Client<Connector, Body>,
    options: Options,
    counters: Counters,
    /// Becomes `true` when the proxy is shutting down.
    stopping: watch::Receiver<bool>,
}

/// Serves the proxy on `listener` until `shutdown` resolves, then stops, with
/// every session recorded into `recorder` — [`serve_traces`] with
/// [`Traces::one`].
///
/// # Errors
///
/// As [`serve_traces`].
pub async fn serve(
    listener: tokio::net::TcpListener,
    recorder: Arc<Recorder>,
    options: Options,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<Unrecorded> {
    serve_traces(listener, Arc::new(Traces::one(recorder)), options, shutdown).await
}

/// Serves the proxy on `listener` until `shutdown` resolves, then stops, with
/// each exchange recorded into the trace `traces` routes it to.
///
/// Stopping cuts every standalone event stream (the answer to a GET) between
/// two chunks the upstream sent — aborting the response, so the client can tell
/// it from a stream the server ended — gives every other request in flight, SSE
/// responses to POSTs included, [`Options::grace`] to finish, and closes every
/// trace with a `transport-close` event — so a client holding a stream open
/// cannot keep the proxy running.
///
/// # Errors
///
/// The server failed.
pub async fn serve_traces(
    listener: tokio::net::TcpListener,
    traces: Arc<Traces>,
    options: Options,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<Unrecorded> {
    let (stop, stopping) = watch::channel(false);
    let grace = options.grace;
    let proxy = Arc::new(Proxy {
        traces,
        client: Client::builder(TokioExecutor::new()).build(options.connector.clone()),
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
    proxy.traces.close_all(LifecycleEvent::TransportClose);
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
/// The platform's root certificates are loaded only for an `https` upstream:
/// where none can be found (a minimal container), a plain `http` upstream must
/// still work. The proxy only ever connects to the one upstream, so an `http`
/// one never uses the TLS settings, and gets none it could trust.
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
fn connector(https: bool) -> io::Result<Connector> {
    #[cfg(feature = "tls")]
    let connector = {
        let builder = hyper_rustls::HttpsConnectorBuilder::new();
        let builder = if https {
            builder.with_provider_and_native_roots(rustls::crypto::ring::default_provider())?
        } else {
            builder.with_tls_config(
                rustls::ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .map_err(io::Error::other)?
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
            )
        };
        builder.https_or_http().enable_http1().build()
    };
    #[cfg(not(feature = "tls"))]
    let connector = {
        let _ = https;
        HttpConnector::new()
    };
    Ok(connector)
}

#[cfg(test)]
mod tests;
