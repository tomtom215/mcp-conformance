// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The host against a server that answers with a body that never ends.
//!
//! The reference host is a test tool that points at servers it does not trust,
//! so a hostile server is its ordinary case. Before rmcp 3.9.0
//! (RUSTSEC-2026-0462) the HTTP client read a non-streaming JSON or error body
//! to the end however large: measured against the 3.7.0 host, a server that
//! sent 3,000 MiB of chunked JSON drove the host to 3,041 MiB resident in under
//! three seconds, so the 25-second `--deadline-secs` bounded time and not
//! memory. These tests pin the bound from outside, against the real
//! executable: a server serves a body of [`SERVER_CAP`] bytes in one shape
//! after another, and each test asserts how many bytes the server managed to
//! write before the host hung up. A client that reads to the end lets the
//! server finish all [`SERVER_CAP`] bytes; one that cuts the body off at its
//! limit lets through that limit plus what the kernel had already buffered, far
//! under [`HUNG_UP_BY`].
//!
//! The server side counts what the *kernel accepted* for the connection, which
//! includes the socket buffers on both ends (a few MiB on loopback), so the
//! threshold is a multiple of the client's limits, not equal to them.
//!
//! Compressed bodies are not covered because there is nothing to expand: the
//! workspace builds `reqwest` with default features off (no decompression
//! feature), so a `Content-Encoding` is passed through undecoded.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How many bytes a hostile server is willing to write to one connection.
const SERVER_CAP: u64 = 192 * 1024 * 1024;

/// The most a connection may have been fed when the host hung up. Three times
/// rmcp's 16 MiB JSON limit and a quarter of [`SERVER_CAP`]: a client that
/// reads to the end writes all of [`SERVER_CAP`].
const HUNG_UP_BY: u64 = 48 * 1024 * 1024;

/// One 64 KiB piece of body.
const PIECE: usize = 64 * 1024;

/// What a hostile server answers every request with.
#[derive(Clone, Copy)]
enum Reply {
    /// `200 application/json`, chunked, whitespace forever (no length).
    EndlessJson,
    /// `500 text/plain`, chunked, whitespace forever: an error body is read
    /// for its message, so it is a separate code path from the JSON one.
    EndlessError,
    /// `200 application/json` declaring a 3 GiB `Content-Length`, then sending.
    DeclaredHuge,
    /// `200 application/json` declaring five bytes, then sending far more.
    DeclaredSmall,
    /// `200 application/json`, chunked, one byte every 50 ms forever.
    Trickle,
}

/// A hostile server on a loopback port, and what it saw.
struct Hostile {
    url: String,
    /// The most bytes any one connection was given.
    most_written: Arc<AtomicU64>,
    /// Connections still being written to.
    active: Arc<AtomicUsize>,
}

impl Hostile {
    fn start(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let most_written = Arc::new(AtomicU64::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let (most, live) = (Arc::clone(&most_written), Arc::clone(&active));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (most, live) = (Arc::clone(&most), Arc::clone(&live));
                live.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    most.fetch_max(serve(stream, reply), Ordering::SeqCst);
                    live.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Self {
            url,
            most_written,
            active,
        }
    }

    /// The most any connection was fed, once every writer has stopped (the
    /// host has exited, so each ends at its next write at the latest).
    fn most_written(&self) -> u64 {
        let until = Instant::now() + Duration::from_secs(10);
        while self.active.load(Ordering::SeqCst) > 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            self.active.load(Ordering::SeqCst),
            0,
            "a connection was still being written to ten seconds after the host exited"
        );
        self.most_written.load(Ordering::SeqCst)
    }
}

/// Reads one request (head and, if it declares one, body) and answers it.
/// Returns how many bytes of body it got onto the wire.
fn serve(mut stream: TcpStream, reply: Reply) -> u64 {
    if !read_request(&mut stream) {
        return 0;
    }
    let head: &[u8] = match reply {
        Reply::EndlessJson | Reply::Trickle => {
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\n\r\n"
        }
        Reply::EndlessError => {
            b"HTTP/1.1 500 Internal Server Error\r\ncontent-type: text/plain\r\ntransfer-encoding: chunked\r\n\r\n"
        }
        Reply::DeclaredHuge => {
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 3221225472\r\n\r\n"
        }
        Reply::DeclaredSmall => {
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 5\r\n\r\n"
        }
    };
    if stream.write_all(head).is_err() {
        return 0;
    }
    let mut written = 0_u64;
    let piece = vec![b' '; PIECE];
    let framed = [format!("{PIECE:x}\r\n").as_bytes(), &piece, b"\r\n"].concat();
    while written < SERVER_CAP {
        let sent = match reply {
            Reply::EndlessJson | Reply::EndlessError => stream.write_all(&framed),
            Reply::DeclaredHuge | Reply::DeclaredSmall => stream.write_all(&piece),
            Reply::Trickle => {
                std::thread::sleep(Duration::from_millis(50));
                stream.write_all(b"1\r\n \r\n")
            }
        };
        if sent.is_err() {
            break;
        }
        written += if matches!(reply, Reply::Trickle) {
            1
        } else {
            PIECE as u64
        };
    }
    written
}

/// Consumes one HTTP request; `false` if the peer went away first.
fn read_request(stream: &mut TcpStream) -> bool {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => buffer.push(byte[0]),
            _ => return false,
        }
    }
    let head = String::from_utf8_lossy(&buffer).to_ascii_lowercase();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body).is_ok()
}

/// Runs the host against `hostile` with a ten-second deadline, so a client
/// that does not cut a body off still ends the test.
fn run_host(hostile: &Hostile, args: &[&str], scenario: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"));
    command.args(["--deadline-secs", "10"]).args(args);
    if let Some(scenario) = scenario {
        command.env("MCP_CONFORMANCE_SCENARIO", scenario);
    }
    command.arg(&hostile.url).output().expect("binary runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn an_endless_json_body_is_cut_off_at_the_clients_limit() {
    let hostile = Hostile::start(Reply::EndlessJson);
    let output = run_host(&hostile, &[], None);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("maximum size"),
        "the diagnostic says the body was too large: {}",
        stderr(&output)
    );
    assert!(hostile.most_written() < HUNG_UP_BY);
}

#[test]
fn an_endless_error_body_is_cut_off_at_the_clients_limit() {
    let hostile = Hostile::start(Reply::EndlessError);
    let output = run_host(&hostile, &[], None);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("500"),
        "the diagnostic names the status: {}",
        stderr(&output)
    );
    assert!(hostile.most_written() < HUNG_UP_BY);
}

#[test]
fn a_huge_declared_length_is_refused_before_the_body_is_read() {
    let hostile = Hostile::start(Reply::DeclaredHuge);
    let output = run_host(&hostile, &[], None);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(hostile.most_written() < HUNG_UP_BY);
}

#[test]
fn a_length_smaller_than_the_body_does_not_extend_the_read() {
    let hostile = Hostile::start(Reply::DeclaredSmall);
    let output = run_host(&hostile, &[], None);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(hostile.most_written() < HUNG_UP_BY);
}

#[test]
fn a_body_that_trickles_forever_is_ended_by_the_deadline() {
    let hostile = Hostile::start(Reply::Trickle);
    let started = Instant::now();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"));
    let output = command
        .args(["--deadline-secs", "2"])
        .arg(&hostile.url)
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(stderr(&output).contains("deadline"), "{}", stderr(&output));
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    assert!(hostile.most_written() < HUNG_UP_BY);
}

#[test]
fn the_sse_retry_scenario_is_bounded_too() {
    let hostile = Hostile::start(Reply::EndlessJson);
    let output = run_host(&hostile, &[], Some("sse-retry"));
    assert_ne!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(hostile.most_written() < HUNG_UP_BY);
}

#[test]
fn the_probe_reads_a_bounded_response() {
    // `--probe` sends raw HTTP through its own client, not rmcp's, so rmcp's
    // limit does not reach it.
    let hostile = Hostile::start(Reply::EndlessJson);
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"));
    let output = command
        .args(["--deadline-secs", "10", "--probe", "--url"])
        .arg(&hostile.url)
        .output()
        .expect("binary runs");
    assert!(hostile.most_written() < HUNG_UP_BY, "{}", stderr(&output));
}
