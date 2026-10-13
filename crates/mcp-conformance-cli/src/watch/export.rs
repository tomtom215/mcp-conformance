// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `--otlp`: spans to a collector, never in the session's way.
//!
//! The judging thread hands each finished span to [`SpanSink::send`], which
//! never waits: the queue is bounded ([`QUEUE`] spans), and a span that finds
//! it full is dropped and counted. A task on the runtime takes spans off the
//! queue in batches — up to [`BATCH`], or whatever arrived within [`LINGER`] of
//! the first — and POSTs each as one OTLP/HTTP JSON request
//! (`Content-Type: application/json`). A collector that is down, slow or
//! refusing costs that batch, counted, and nothing else: no retry queue grows,
//! and the session never knew the collector existed.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, Uri, header};
use mcp_conformance_cli::otlp::{self, Span};
use mcp_trace_capture::http::Client;
use tokio::sync::mpsc;

/// Spans the queue holds before it drops.
pub(super) const QUEUE: usize = 4096;
/// Spans per request at most.
pub(super) const BATCH: usize = 512;
/// How long a batch waits for more spans after its first.
pub(super) const LINGER: Duration = Duration::from_secs(1);
/// How long connecting to the collector may take.
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long one request may take, connection included.
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Where spans go.
#[derive(Debug, Clone)]
pub(super) struct Endpoint {
    /// The traces URL: the given base with `/v1/traces` appended.
    pub(super) uri: Uri,
    client: Client,
}

impl Endpoint {
    /// The collector `text` names: an `http(s)` base URL, or one ending in
    /// `/v1/traces`.
    ///
    /// # Errors
    ///
    /// Not an absolute `http(s)` URL, credentials in it, or (for `https`) no
    /// root certificates. The URL is never repeated: it can carry a key.
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let base: Uri = text
            .parse()
            .map_err(|_| "not a URL (expected e.g. http://localhost:4318)".to_owned())?;
        let (Some(scheme), Some(authority)) = (base.scheme_str(), base.authority()) else {
            return Err("not an absolute http:// or https:// URL".to_owned());
        };
        if authority.as_str().contains('@') {
            return Err(
                "the URL carries credentials (user:password@), which watch does not send"
                    .to_owned(),
            );
        }
        let path = base.path().trim_end_matches('/');
        let path = if path.ends_with("/v1/traces") {
            path.to_owned()
        } else {
            format!("{path}/v1/traces")
        };
        let query = base
            .query()
            .map_or_else(String::new, |query| format!("?{query}"));
        let uri: Uri = format!("{scheme}://{authority}{path}{query}")
            .parse()
            .map_err(|_| "not a URL (expected e.g. http://localhost:4318)".to_owned())?;
        let client = mcp_trace_capture::http::client(&uri, CONNECT_TIMEOUT)?;
        Ok(Self { uri, client })
    }

    /// The URL as announced: without its query, which can carry a key.
    pub(super) fn shown(&self) -> String {
        format!(
            "{}://{}{}",
            self.uri.scheme_str().unwrap_or("http"),
            self.uri
                .authority()
                .map_or("", |authority| authority.as_str()),
            self.uri.path()
        )
    }

    /// Whether spans to it cross the network in the clear.
    pub(super) fn is_plaintext_remote(&self) -> bool {
        self.uri.scheme_str() == Some("http") && !crate::proxy::is_loopback(&self.uri)
    }
}

/// The judging thread's end of the queue.
#[derive(Debug)]
pub(super) struct SpanSink {
    queue: mpsc::Sender<Span>,
    dropped: Arc<AtomicU64>,
}

impl SpanSink {
    /// Queues `span`, or drops and counts it when the queue is full.
    pub(super) fn send(&self, span: Span) {
        if self.queue.try_send(span).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// How the export went.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Exported {
    /// Spans a collector accepted (a 2xx).
    pub(super) sent: u64,
    /// Spans in requests that failed.
    pub(super) failed: u64,
    /// Spans dropped because the queue was full.
    pub(super) dropped: u64,
    /// The first failure, for the summary.
    pub(super) error: Option<String>,
}

/// Starts the export task on the current runtime.
pub(super) fn start(endpoint: Endpoint) -> (SpanSink, tokio::task::JoinHandle<Exported>) {
    let (queue, spans) = mpsc::channel(QUEUE);
    let dropped = Arc::new(AtomicU64::new(0));
    let task = tokio::spawn(export(spans, endpoint, Arc::clone(&dropped)));
    (SpanSink { queue, dropped }, task)
}

async fn export(
    mut spans: mpsc::Receiver<Span>,
    endpoint: Endpoint,
    dropped: Arc<AtomicU64>,
) -> Exported {
    let mut exported = Exported::default();
    while let Some(first) = spans.recv().await {
        let mut batch = vec![first];
        let linger = tokio::time::sleep(LINGER);
        tokio::pin!(linger);
        while batch.len() < BATCH {
            tokio::select! {
                more = spans.recv() => match more {
                    Some(span) => batch.push(span),
                    None => break,
                },
                () = &mut linger => break,
            }
        }
        let count = u64::try_from(batch.len()).unwrap_or(u64::MAX);
        match post(&endpoint, batch).await {
            Ok(()) => exported.sent += count,
            Err(error) => {
                exported.failed += count;
                exported.error.get_or_insert(error);
            }
        }
    }
    exported.dropped = dropped.load(Ordering::Relaxed);
    exported
}

/// One `POST /v1/traces`.
async fn post(endpoint: &Endpoint, spans: Vec<Span>) -> Result<(), String> {
    let body = serde_json::to_vec(&otlp::Request::new(spans))
        .map_err(|error| format!("cannot encode spans: {error}"))?;
    let request = Request::post(endpoint.uri.clone())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .map_err(|error| format!("cannot build the request: {error}"))?;
    let response = tokio::time::timeout(REQUEST_TIMEOUT, endpoint.client.request(request))
        .await
        .map_err(|_| format!("no answer within {}s", REQUEST_TIMEOUT.as_secs()))?
        .map_err(|error| format!("cannot reach the collector: {}", causes(&error)))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("the collector answered {status}"))
    }
}

/// An error and its sources, as one line.
fn causes(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn an_endpoint_gets_the_traces_path_once_and_is_shown_without_its_query() {
        for (given, uri) in [
            ("http://localhost:4318", "http://localhost:4318/v1/traces"),
            ("http://localhost:4318/", "http://localhost:4318/v1/traces"),
            ("http://c:4318/otel", "http://c:4318/otel/v1/traces"),
            ("http://c:4318/v1/traces", "http://c:4318/v1/traces"),
            (
                "https://c/v1/traces?key=s3cret",
                "https://c/v1/traces?key=s3cret",
            ),
        ] {
            let endpoint = Endpoint::parse(given).unwrap();
            assert_eq!(endpoint.uri.to_string(), uri, "{given}");
            assert!(!endpoint.shown().contains("s3cret"));
        }
        assert!(
            Endpoint::parse("http://10.0.0.1:4318")
                .unwrap()
                .is_plaintext_remote()
        );
        assert!(
            !Endpoint::parse("http://127.0.0.1:4318")
                .unwrap()
                .is_plaintext_remote()
        );
        assert!(!Endpoint::parse("https://c").unwrap().is_plaintext_remote());
    }

    #[test]
    fn a_bad_endpoint_is_refused_without_being_repeated() {
        for bad in [
            "localhost:4318",
            "/v1/traces",
            "ftp://c/x",
            "http://u:s3cret@c/",
            "not a url s3cret",
        ] {
            let error = Endpoint::parse(bad).unwrap_err();
            assert!(!error.contains("s3cret"), "{error}");
        }
    }
}
