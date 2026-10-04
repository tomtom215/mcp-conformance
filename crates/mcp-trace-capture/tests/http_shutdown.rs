// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Stopping the HTTP proxy while a client is connected: an MCP client holds a
//! GET event stream open for its whole session, so "Ctrl-C to stop" has to work
//! with streams open — within a bounded time, with the trace closed and the
//! summary written.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead as _, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-shutdown-{name}-{}.jsonl",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// An upstream that answers `GET` with an event stream that never ends (a
/// comment every 100 ms) and `POST` with a JSON body it never finishes.
fn endless_upstream() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || {
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    request.push(byte[0]);
                }
                if request.starts_with(b"GET") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                          transfer-encoding: chunked\r\n\r\n",
                    );
                    loop {
                        if stream.write_all(b"3\r\n:\n\n\r\n").is_err() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                      content-length: 100\r\n\r\n{\"jsonrpc\":",
                );
                std::thread::sleep(Duration::from_secs(60));
            });
        }
    });
    address
}

struct Proxy(Child);

impl Drop for Proxy {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

impl Proxy {
    fn start(trace: &std::path::Path, upstream: &str) -> (Self, String, Receiver<String>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
            .args([
                "-o",
                trace.to_str().unwrap(),
                "http",
                "--listen",
                "127.0.0.1:0",
            ])
            .args(["--upstream", &format!("http://{upstream}")])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (address_tx, address_rx) = std::sync::mpsc::channel();
        let (lines_tx, lines_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                if let Some(rest) = line.split("listening on http://").nth(1) {
                    let _ = address_tx.send(rest.split(',').next().unwrap().to_owned());
                }
                let _ = lines_tx.send(line);
            }
        });
        let address = address_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the proxy announces its address");
        (Self(child), address, lines_rx)
    }

    fn signal(&self, name: &str) {
        let status = Command::new("kill")
            .args([&format!("-{name}"), &self.0.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn wait_within(&mut self, limit: Duration) -> ExitStatus {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the proxy did not stop within {limit:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Opens a request through the proxy and reads until the response head is in.
fn open(address: &str, request: &str) -> TcpStream {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        assert_eq!(
            stream.read(&mut byte).unwrap(),
            1,
            "a response head arrives"
        );
        head.push(byte[0]);
    }
    assert!(
        head.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&head)
    );
    stream
}

#[test]
fn ctrl_c_stops_the_proxy_promptly_with_an_event_stream_open() {
    let upstream = endless_upstream();
    let trace = scratch("sse");
    let (mut proxy, address, lines) = Proxy::start(&trace, &upstream);
    let mut stream = open(
        &address,
        "GET /mcp HTTP/1.1\r\nhost: localhost\r\naccept: text/event-stream\r\n\r\n",
    );
    let started = Instant::now();
    proxy.signal("INT");
    let status = proxy.wait_within(Duration::from_secs(10));
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "an open event stream is ended at shutdown, not waited out: {:?}",
        started.elapsed()
    );
    // The client sees its stream end.
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest);
    std::thread::sleep(Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    assert!(
        stderr.iter().any(|line| line.contains("recorded")),
        "the summary is written: {stderr:?}"
    );
    let text = std::fs::read_to_string(&trace).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["event"], "transport-close", "{text}");
    std::fs::remove_file(&trace).ok();
}

/// A response the upstream never finishes is not waited for beyond the grace
/// period — and a second signal does not wait for that either.
#[test]
fn a_response_that_never_completes_does_not_hold_the_proxy_open() {
    let upstream = endless_upstream();
    let post = "POST /mcp HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\n\
                content-length: 2\r\n\r\n{}";

    let trace = scratch("hung");
    let (mut proxy, address, _lines) = Proxy::start(&trace, &upstream);
    let _held = {
        let mut stream = TcpStream::connect(&address).unwrap();
        stream.write_all(post.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        stream
    };
    let started = Instant::now();
    proxy.signal("TERM");
    let status = proxy.wait_within(Duration::from_secs(10));
    assert!(status.code().is_some(), "{status:?}");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    let text = std::fs::read_to_string(&trace).unwrap();
    assert!(
        text.lines().last().unwrap().contains("transport-close"),
        "{text}"
    );
    std::fs::remove_file(&trace).ok();

    let trace = scratch("hung-twice");
    let (mut proxy, address, _lines) = Proxy::start(&trace, &upstream);
    let _held = {
        let mut stream = TcpStream::connect(&address).unwrap();
        stream.write_all(post.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        stream
    };
    let started = Instant::now();
    proxy.signal("TERM");
    std::thread::sleep(Duration::from_millis(200));
    proxy.signal("TERM");
    let status = proxy.wait_within(Duration::from_secs(10));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the second signal stopped the wait: {:?}",
        started.elapsed()
    );
    assert_eq!(status.code(), Some(128 + 15), "{status:?}");
    let text = std::fs::read_to_string(&trace).unwrap();
    assert!(
        text.lines().last().unwrap().contains("transport-close"),
        "{text}"
    );
    std::fs::remove_file(&trace).ok();
}
