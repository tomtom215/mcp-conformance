// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A trace per session through the HTTP proxy, over real sockets: client
//! sessions — interleaved, and one after another — each land in their own
//! numbered file, `seq` from 0, existing files untouched.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::response::Response;
use axum::routing::post;
use mcp_trace_capture::http;
use mcp_trace_capture::numbered::Numbered;
use mcp_trace_capture::traces::Traces;

async fn bounded(body: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(std::time::Duration::from_secs(20), body)
        .await
        .expect("the test finished within 20 s");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcp-trace-capture-it-sessions-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A server answering every request with an empty result; with `assign`, it
/// gives each `initialize` a fresh `Mcp-Session-Id` (`s-1`, `s-2`, …).
async fn upstream(assign: bool) -> SocketAddr {
    let next = Arc::new(AtomicU64::new(1));
    let app = axum::Router::new().route(
        "/mcp",
        post(move |body: String| async move {
            let request: serde_json::Value = serde_json::from_str(&body).unwrap();
            let result = serde_json::json!({"jsonrpc": "2.0", "id": request["id"], "result": {}});
            let mut response = Response::builder().header("content-type", "application/json");
            if assign && request["method"] == "initialize" {
                let id = next.fetch_add(1, Ordering::Relaxed);
                response = response.header("mcp-session-id", format!("s-{id}"));
            }
            response.body(Body::from(result.to_string())).unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// The proxy in front of `upstream`, a trace per session into `dir`; returns its
/// address, its sessions, and a handle that stops it.
async fn proxy(
    upstream: SocketAddr,
    dir: &Path,
) -> (
    SocketAddr,
    Arc<Traces>,
    impl FnOnce() -> tokio::task::JoinHandle<std::io::Result<http::Unrecorded>>,
) {
    let numbered = Numbered::parse(&dir.join("{session}.jsonl"))
        .unwrap()
        .unwrap();
    let sessions = Arc::new(Traces::per_session(numbered, 1 << 20));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let upstream = format!("http://{upstream}").parse().unwrap();
    let options = http::Options::new(upstream, 1 << 20).unwrap();
    let task = tokio::spawn(http::serve_traces(
        listener,
        Arc::clone(&sessions),
        options,
        async {
            stopped.await.ok();
        },
    ));
    (address, sessions, move || {
        stop.send(()).ok();
        task
    })
}

/// Sends one request, naming `session` if given; returns the session id the
/// response assigned, if any.
async fn send(address: SocketAddr, id: u64, method: &str, session: Option<&str>) -> Option<String> {
    let mut request = reqwest::Client::new()
        .post(format!("http://{address}/mcp"))
        .header("content-type", "application/json")
        .body(format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{}}}}"#
        ));
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = request.send().await.unwrap();
    assert_eq!(response.status(), 200);
    let assigned = response
        .headers()
        .get("mcp-session-id")
        .map(|value| value.to_str().unwrap().to_owned());
    response.text().await.unwrap();
    assigned
}

/// Each line's `seq`, and the JSON-RPC ids of the messages in it.
fn read(path: &Path) -> (Vec<u64>, Vec<u64>) {
    let events: Vec<serde_json::Value> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let seqs = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    let ids = events
        .iter()
        .filter_map(|e| e["payload"]["id"].as_u64())
        .collect();
    (seqs, ids)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interleaved_and_successive_sessions_each_get_their_own_trace() {
    bounded(async {
        let dir = scratch("http");
        // A trace already there is never touched: numbering starts past it.
        std::fs::write(dir.join("001.jsonl"), "keep me\n").unwrap();
        let (address, sessions, stop) = proxy(upstream(true).await, &dir).await;

        // Two clients interleaved; the ids say whose each message is.
        let a = send(address, 10, "initialize", None).await.unwrap();
        let b = send(address, 20, "initialize", None).await.unwrap();
        send(address, 11, "ping", Some(&a)).await;
        send(address, 21, "ping", Some(&b)).await;
        send(address, 12, "tools/list", Some(&a)).await;
        // Then a third, after both.
        let c = send(address, 30, "initialize", None).await.unwrap();
        send(address, 31, "ping", Some(&c)).await;
        stop().await.unwrap().unwrap();

        let finished = sessions.finish();
        assert!(finished.iter().all(|done| done.summary.is_complete()));
        let paths: Vec<PathBuf> = finished.into_iter().map(|d| d.path.unwrap()).collect();
        assert_eq!(
            paths,
            ["002.jsonl", "003.jsonl", "004.jsonl"].map(|name| dir.join(name))
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("001.jsonl")).unwrap(),
            "keep me\n"
        );
        for (path, expected) in paths.iter().zip([
            vec![10, 10, 11, 11, 12, 12],
            vec![20, 20, 21, 21],
            vec![30, 30, 31, 31],
        ]) {
            let (seqs, ids) = read(path);
            assert_eq!(ids, expected, "{}", path.display());
            // Request and response each an `http` event and a message, then the
            // `transport-close` the proxy ends every trace with as it stops.
            let count = u64::try_from(expected.len() * 2 + 1).unwrap();
            assert_eq!(seqs, (0..count).collect::<Vec<_>>(), "{}", path.display());
            let text = std::fs::read_to_string(path).unwrap();
            let last: serde_json::Value =
                serde_json::from_str(text.lines().last().unwrap()).unwrap();
            assert_eq!(last["event"], "transport-close", "{}", path.display());
        }
        std::fs::remove_dir_all(&dir).ok();
    })
    .await;
}

/// Without session ids, an `initialize` still begins a session, and what
/// follows it belongs to it; traffic with neither (`2026-07-28`) is one session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_session_ids_initialize_alone_divides_sessions() {
    bounded(async {
        let dir = scratch("no-ids");
        let (address, sessions, stop) = proxy(upstream(false).await, &dir).await;
        send(address, 1, "server/discover", None).await;
        send(address, 2, "tools/list", None).await;
        send(address, 10, "initialize", None).await;
        send(address, 11, "ping", None).await;
        stop().await.unwrap().unwrap();
        assert_eq!(sessions.finish().len(), 2);
        assert_eq!(read(&dir.join("001.jsonl")).1, [1, 1, 2, 2]);
        assert_eq!(read(&dir.join("002.jsonl")).1, [10, 10, 11, 11]);
        std::fs::remove_dir_all(&dir).ok();
    })
    .await;
}

/// The binary's proxy, run to completion against `upstream` with `-o output`,
/// while `traffic` runs against its address; returns its exit code and every
/// line it wrote to stderr — read to the end, after it exited.
#[cfg(all(unix, feature = "cli"))]
async fn run_binary_proxy<F, Fut>(upstream: SocketAddr, output: &Path, traffic: F) -> (i32, String)
where
    F: FnOnce(SocketAddr) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    use std::io::Read as _;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
        .args([
            "-o",
            output.to_str().unwrap(),
            "http",
            "--listen",
            "127.0.0.1:0",
        ])
        .args(["--upstream", &format!("http://{upstream}")])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    // Read byte by byte up to the announcement, so nothing after it is consumed
    // here; the rest is read to EOF once the proxy has exited.
    let mut head = Vec::new();
    let address = loop {
        let mut byte = [0_u8];
        assert_eq!(
            stderr.read(&mut byte).unwrap(),
            1,
            "stderr closed early: {head:?}"
        );
        head.push(byte[0]);
        let text = String::from_utf8_lossy(&head);
        if let Some(rest) = text.split("listening on http://").nth(1)
            && let Some((address, _)) = rest.split_once(',')
        {
            break address.parse::<SocketAddr>().unwrap();
        }
    };
    traffic(address).await;
    let status = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    tokio::task::spawn_blocking(move || {
        let code = child.wait().unwrap().code().unwrap();
        let mut rest = String::new();
        stderr.read_to_string(&mut rest).unwrap();
        (code, String::from_utf8_lossy(&head).into_owned() + &rest)
    })
    .await
    .unwrap()
}

#[cfg(all(unix, feature = "cli"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_binary_writes_a_trace_per_session_or_warns_that_one_holds_several() {
    bounded(async {
        let dir = scratch("binary");
        let upstream = upstream(true).await;
        let two_sessions = |address| async move {
            for base in [10, 20] {
                let id = send(address, base, "initialize", None).await.unwrap();
                send(address, base + 1, "ping", Some(&id)).await;
            }
        };

        let (code, stderr) =
            run_binary_proxy(upstream, &dir.join("{session}.jsonl"), two_sessions).await;
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(read(&dir.join("001.jsonl")).1, [10, 10, 11, 11]);
        assert_eq!(read(&dir.join("002.jsonl")).1, [20, 20, 21, 21]);
        for name in ["001", "002"] {
            let path = dir.join(format!("{name}.jsonl"));
            assert!(
                stderr.contains(&format!("recorded 9 event(s) to {}", path.display())),
                "{stderr}"
            );
        }
        assert!(!stderr.contains("warning: this trace holds"), "{stderr}");

        let single = dir.join("all.jsonl");
        let (code, stderr) = run_binary_proxy(upstream, &single, two_sessions).await;
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(read(&single).1, [10, 10, 11, 11, 20, 20, 21, 21]);
        // One warning, from the trace's own session count, naming the fix.
        assert_eq!(
            stderr
                .matches("warning: this trace holds 2 sessions")
                .count(),
            1,
            "{stderr}"
        );
        assert!(stderr.contains("put {session} in -o"), "{stderr}");

        // A proxy no client used says so rather than naming no file.
        let idle = |_| async {};
        let (code, stderr) =
            run_binary_proxy(upstream, &dir.join("idle-{session}.jsonl"), idle).await;
        assert_eq!(code, 0, "{stderr}");
        assert!(stderr.contains("no session began"), "{stderr}");
        std::fs::remove_dir_all(&dir).ok();
    })
    .await;
}
