// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The recorder's HTTP proxy, in process: each session the suite (or a client
//! under test) begins is written to its own numbered trace in a directory.
//!
//! The proxy is `mcp-trace-capture http`'s, with its guarantees: bytes forwarded
//! unchanged, each message recorded before the bytes completing it are passed
//! on, listening on loopback only (an OS-assigned port).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::http::Uri;
use mcp_conformance_core::trace::DEFAULT_MAX_LINE_BYTES;
use mcp_trace_capture::DEFAULT_MAX_MESSAGE;
use mcp_trace_capture::http::{self, Options};
use mcp_trace_capture::numbered::Numbered;
use mcp_trace_capture::traces::Traces;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// A running proxy.
#[derive(Debug)]
pub(crate) struct Proxy {
    /// Where it listens.
    pub(crate) address: SocketAddr,
    stop: oneshot::Sender<()>,
    serving: JoinHandle<std::io::Result<http::Unrecorded>>,
    traces: Arc<Traces>,
}

/// An `http(s)://` URL split into the proxy's upstream (scheme and authority)
/// and what a client asks the proxy for (the path and query), so the proxy
/// maps every path one to one and the suite sees the server's own paths.
///
/// # Errors
///
/// Not an absolute `http`/`https` URL. The message never repeats the URL,
/// whose query can carry a key.
pub(crate) fn split(url: &str) -> Result<(Uri, String), String> {
    let parsed: Uri = url
        .parse()
        .map_err(|_| "not a URL (expected e.g. http://localhost:3000/mcp)".to_owned())?;
    let (Some(scheme), Some(authority)) = (parsed.scheme_str(), parsed.authority()) else {
        return Err("not an absolute http:// or https:// URL".to_owned());
    };
    if !matches!(scheme, "http" | "https") {
        return Err("not an absolute http:// or https:// URL".to_owned());
    }
    let upstream: Uri = format!("{scheme}://{authority}")
        .parse()
        .map_err(|_| "not an absolute http:// or https:// URL".to_owned())?;
    let path = parsed
        .path_and_query()
        .map_or_else(|| "/".to_owned(), ToString::to_string);
    Ok((upstream, path))
}

/// Whether `upstream` names this machine: `localhost`, `127.0.0.0/8` or `::1`.
pub(crate) fn is_loopback(upstream: &Uri) -> bool {
    let host = upstream.host().unwrap_or_default();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Starts a proxy to `upstream` on `127.0.0.1:0`, recording each session to the
/// next `NNN.jsonl` in `dir` (which must exist).
///
/// # Errors
///
/// The upstream is refused (see [`Options::new`]), or the listener cannot bind.
pub(crate) async fn start(upstream: Uri, preserve_host: bool, dir: &Path) -> Result<Proxy, String> {
    let mut options = Options::new(upstream, DEFAULT_MAX_MESSAGE)?;
    options.preserve_host = preserve_host;
    let numbered = Numbered::parse(&dir.join("{session}.jsonl"))?
        .ok_or_else(|| format!("{}: cannot number traces there", dir.display()))?;
    let traces = Arc::new(Traces::per_session(numbered, DEFAULT_MAX_LINE_BYTES));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("cannot listen on 127.0.0.1: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("cannot read the listening address: {error}"))?;
    let (stop, stopped) = oneshot::channel::<()>();
    let serving = tokio::spawn(http::serve_traces(
        listener,
        Arc::clone(&traces),
        options,
        async {
            let _ = stopped.await;
        },
    ));
    Ok(Proxy {
        address,
        stop,
        serving,
        traces,
    })
}

impl Proxy {
    /// Stops the proxy (open exchanges get the recorder's grace period),
    /// flushes every trace, and says on stderr what was not recorded.
    ///
    /// # Errors
    ///
    /// A trace is incomplete (a write failed), or the proxy failed.
    pub(crate) async fn stop(self) -> Result<(), String> {
        let _ = self.stop.send(());
        let unrecorded = self
            .serving
            .await
            .map_err(|error| format!("the recording proxy failed: {error}"))?
            .map_err(|error| format!("the recording proxy failed: {error}"))?;
        for note in unrecorded_notes(unrecorded) {
            eprintln!("mcp-conformance: {note}");
        }
        let mut incomplete = Vec::new();
        for finished in self.traces.finish() {
            if !finished.summary.is_complete() {
                incomplete.push(
                    finished
                        .path
                        .map_or_else(String::new, |path| path.display().to_string()),
                );
            }
        }
        if incomplete.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "a trace write failed, so these traces are incomplete: {}",
                incomplete.join(", ")
            ))
        }
    }
}

/// What the proxy forwarded without recording, when anything: findings of the
/// recording, not of the subject, which [`Proxy::stop`] puts on stderr.
fn unrecorded_notes(unrecorded: http::Unrecorded) -> Vec<String> {
    let mut notes = Vec::new();
    let left_out = unrecorded.not_json
        + unrecorded.altered
        + unrecorded.oversized
        + unrecorded.over_budget
        + unrecorded.other_events;
    if left_out > 0 {
        notes.push(format!(
            "{left_out} message(s) were forwarded but not recorded (not JSON, over the size \
             limit, or not representable); see mcp-trace-capture's README"
        ));
    }
    if unrecorded.upstream_failures > 0 {
        notes.push(format!(
            "{} request(s) could not reach the server and were answered 502 by the recording \
             proxy",
            unrecorded.upstream_failures
        ));
    }
    notes
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_url_splits_into_upstream_and_path() {
        let (upstream, path) = split("http://localhost:3000/mcp?key=1").unwrap();
        assert_eq!(upstream.to_string(), "http://localhost:3000/");
        assert_eq!(path, "/mcp?key=1");
        let (upstream, path) = split("https://example.com").unwrap();
        assert_eq!(upstream.to_string(), "https://example.com/");
        assert_eq!(path, "/");
    }

    #[test]
    fn what_is_not_an_http_url_is_refused_without_repeating_it() {
        for bad in [
            "localhost:3000/mcp",
            "/mcp",
            "ftp://host/x",
            "http://",
            "not a url ?key=s3cret",
        ] {
            let error = split(bad).unwrap_err();
            assert!(!error.contains("s3cret"), "{error}");
            assert!(error.contains("http"), "{bad}: {error}");
        }
    }

    #[test]
    fn loopback_is_localhost_and_the_loopback_addresses() {
        for host in [
            "http://localhost:1",
            "http://LOCALHOST",
            "http://127.0.0.1:2",
            "http://127.8.0.1",
            "http://[::1]:3",
        ] {
            assert!(is_loopback(&host.parse().unwrap()), "{host}");
        }
        for host in [
            "http://example.com",
            "http://10.0.0.1",
            "http://[::2]",
            "http://localhost.example",
        ] {
            assert!(!is_loopback(&host.parse().unwrap()), "{host}");
        }
    }

    #[test]
    fn what_was_not_recorded_is_counted_and_noted_once_each() {
        let none: Vec<String> = Vec::new();
        assert_eq!(unrecorded_notes(http::Unrecorded::default()), none);
        let mut unrecorded = http::Unrecorded::default();
        unrecorded.not_json = 1;
        unrecorded.altered = 2;
        unrecorded.oversized = 3;
        unrecorded.over_budget = 4;
        unrecorded.other_events = 5;
        unrecorded.upstream_failures = 6;
        assert_eq!(
            unrecorded_notes(unrecorded),
            [
                "15 message(s) were forwarded but not recorded (not JSON, over the size limit, \
                 or not representable); see mcp-trace-capture's README",
                "6 request(s) could not reach the server and were answered 502 by the recording \
                 proxy"
            ]
        );
        let mut one = http::Unrecorded::default();
        one.other_events = 1;
        assert_eq!(unrecorded_notes(one).len(), 1);
        assert!(unrecorded_notes(one)[0].starts_with("1 message(s)"));
        let mut one = http::Unrecorded::default();
        one.upstream_failures = 1;
        assert_eq!(unrecorded_notes(one).len(), 1);
        assert!(unrecorded_notes(one)[0].starts_with("1 request(s)"));
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mcp-conformance-proxy-{name}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An upstream nothing listens on: a loopback port just given back.
    fn refusing() -> Uri {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        format!("http://127.0.0.1:{port}").parse().unwrap()
    }

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;

    #[tokio::test]
    async fn stopping_flushes_the_traces_and_names_any_a_write_failed_on() {
        let client = reqwest::Client::new();

        let dir = scratch("complete");
        let proxy = start(refusing(), true, &dir).await.unwrap();
        let url = format!("http://{}/mcp", proxy.address);
        let answered = client.post(&url).body(INITIALIZE).send().await.unwrap();
        assert_eq!(answered.status(), 502);
        proxy.stop().await.unwrap();
        let trace = std::fs::read_to_string(dir.join("001.jsonl")).unwrap();
        assert!(trace.contains(r#""method":"initialize""#), "{trace}");
        std::fs::remove_dir_all(&dir).ok();

        // The session's file goes away after its first write: the next write
        // to it fails, and the trace is incomplete.
        let dir = scratch("incomplete");
        let proxy = start(refusing(), true, &dir).await.unwrap();
        let url = format!("http://{}/mcp", proxy.address);
        client.post(&url).body(INITIALIZE).send().await.unwrap();
        let path = dir.join("001.jsonl");
        std::fs::remove_file(&path).unwrap();
        let ping = r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;
        client.post(&url).body(ping).send().await.unwrap();
        let error = proxy.stop().await.unwrap_err();
        assert_eq!(
            error,
            format!(
                "a trace write failed, so these traces are incomplete: {}",
                path.display()
            )
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
