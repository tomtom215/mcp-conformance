// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How the proxy connects to the upstream: plain or TLS, and within a bounded
//! time.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::http::uri::Uri;
use hyper_util::client::legacy::connect::HttpConnector;

#[cfg(feature = "tls")]
pub(super) type Connector = hyper_rustls::HttpsConnector<HttpConnector>;
#[cfg(not(feature = "tls"))]
pub(super) type Connector = HttpConnector;

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
pub(super) fn connector(https: bool) -> io::Result<Connector> {
    #[cfg(feature = "tls")]
    let connector = {
        let builder = hyper_rustls::HttpsConnectorBuilder::new();
        let builder = if https {
            builder.with_provider_and_native_roots(rustls::crypto::ring::default_provider())?
        } else {
            builder.with_tls_config(
                rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
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

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// `error` as a [`BoxError`] — which, with the `tls` feature, it already is.
fn boxed(error: impl Into<BoxError>) -> BoxError {
    error.into()
}

/// The upstream connector, with a bound on how long connecting may take.
///
/// The bound includes the TLS handshake. Without one, an upstream that never completes the handshake (a
/// full accept queue, a firewall that drops packets) held each request until
/// the operating system gave up, minutes later.
#[derive(Debug, Clone)]
pub struct Timed {
    pub(super) inner: Connector,
    pub(super) timeout: Duration,
}

impl tower::Service<Uri> for Timed {
    type Response = <Connector as tower::Service<Uri>>::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        self.inner.poll_ready(context).map_err(boxed)
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let connecting = self.inner.call(uri);
        let timeout = self.timeout;
        Box::pin(async move {
            tokio::time::timeout(timeout, connecting).await.map_or_else(
                |_| {
                    Err(boxed(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("no connection within {}s", timeout.as_secs_f32()),
                    )))
                },
                |connected| connected.map_err(boxed),
            )
        })
    }
}
