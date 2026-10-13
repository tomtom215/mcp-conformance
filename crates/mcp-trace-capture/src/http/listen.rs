// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The proxy's side facing the client: accepting connections and serving each
//! with HTTP/1.1.
//!
//! Its own accept loop, not `axum::serve`, for what that one does not offer: a
//! bound on how long a client may take to send a request's head
//! ([`Options::header_read_timeout`](super::Options::header_read_timeout)). A
//! client that opened connections and trickled header bytes into them
//! (slowloris) held each one, and its task, open for as long as it liked.

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::server::graceful::GracefulShutdown;

use super::Proxy;

/// How long the proxy waits after a failed `accept` that is not about one
/// connection (too many open files, say) before trying again.
const ACCEPT_BACKOFF: Duration = Duration::from_secs(1);

/// Serves `proxy` on `listener` until `shutdown` resolves; then stops
/// accepting, lets each open connection finish the exchange it is in (an idle
/// one is closed), and returns once every one has.
pub(super) async fn serve(
    listener: tokio::net::TcpListener,
    proxy: Arc<Proxy>,
    shutdown: impl Future<Output = ()>,
) {
    let graceful = GracefulShutdown::new();
    let header_read_timeout = proxy.options.header_read_timeout;
    tokio::pin!(shutdown);
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(error) => {
                    if !is_connection_error(&error) {
                        eprintln!("mcp-trace-capture: cannot accept a connection: {error}");
                        tokio::time::sleep(ACCEPT_BACKOFF).await;
                    }
                    continue;
                }
            },
            () = &mut shutdown => break,
        };
        let proxy = Arc::clone(&proxy);
        let service = hyper::service::service_fn(move |request: hyper::Request<_>| {
            let proxy = Arc::clone(&proxy);
            async move {
                Ok::<_, Infallible>(super::relay::forward(proxy, request.map(Body::new)).await)
            }
        });
        let mut builder = http1::Builder::new();
        builder
            .timer(TokioTimer::new())
            .header_read_timeout(header_read_timeout);
        let connection = graceful.watch(builder.serve_connection(TokioIo::new(stream), service));
        tokio::spawn(async move {
            // A connection that fails — a client gone, a head too slow or
            // malformed — is that client's business; hyper answered what it
            // could.
            let _ = connection.await;
        });
    }
    drop(listener);
    graceful.shutdown().await;
}

/// Whether a failed `accept` concerns only the connection being accepted —
/// the next one may well succeed at once.
fn is_connection_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}
