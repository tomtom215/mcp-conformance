// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `http` subcommand as a user meets it: what it checks before it creates a
//! trace, what it says when it starts, and what it says when it stops.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-cli-http-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// A running proxy, killed if a test ends without it having exited.
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
    /// Starts `command` (an `http` invocation) and waits for it to announce its
    /// address; returns it, the address and its stderr lines so far and to come.
    fn start(mut command: Command) -> (Self, String, Receiver<String>) {
        let mut child = command.stderr(Stdio::piped()).spawn().unwrap();
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

    /// Sends SIGINT and waits for the exit status.
    fn interrupt(&mut self) -> std::process::ExitStatus {
        let status = Command::new("kill")
            .args(["-INT", &self.0.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "the proxy did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `http` with `-o trace`, listening on a free loopback port.
fn http(trace: &Path, upstream: &str) -> Command {
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "http",
        "--listen",
        "127.0.0.1:0",
        "--upstream",
        upstream,
    ]);
    command
}

/// A directory and file holding no certificate, for `SSL_CERT_DIR` and
/// `SSL_CERT_FILE`: a platform with no root certificates to load.
fn no_root_certificates(command: &mut Command) -> PathBuf {
    let dir = scratch("no-certs");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("empty.pem");
    std::fs::write(&file, "").unwrap();
    command
        .env("SSL_CERT_FILE", &file)
        .env("SSL_CERT_DIR", &dir);
    dir
}

/// Root certificates are an `https` upstream's concern: a plain `http` one
/// works where none can be loaded (a minimal container), and an `https` one
/// that needs them is refused before a trace file exists. Both used to fail
/// after the trace was created and the address announced, leaving an empty file
/// the next run refused to overwrite.
#[test]
fn root_certificates_are_loaded_for_an_https_upstream_only_and_before_the_trace() {
    let trace = scratch("plain.jsonl");
    let mut command = http(&trace, "http://127.0.0.1:9/mcp");
    let dir = no_root_certificates(&mut command);
    let (mut proxy, _, lines) = Proxy::start(command);
    assert_eq!(proxy.interrupt().code(), Some(0));
    std::thread::sleep(Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    assert!(
        !stderr.iter().any(|line| line.contains("certificates")),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();

    if cfg!(feature = "tls") {
        let trace = scratch("tls.jsonl");
        let mut command = http(&trace, "https://127.0.0.1:9/mcp");
        no_root_certificates(&mut command);
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("root certificates"), "{stderr}");
        assert!(!stderr.contains("listening on"), "{stderr}");
        assert!(!trace.exists(), "the refused run left {}", trace.display());
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// An upstream answering every request with `respond(own address, path)`;
/// returns its address.
fn upstream(respond: fn(&str, &str) -> String) -> String {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let own = address.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if stream.read(&mut byte).unwrap_or(0) == 0 {
                    break;
                }
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
            let _ = stream.write_all(respond(&own, &path).as_bytes());
        }
    });
    address
}

/// Sends `GET path` through the proxy at `address`; returns the response.
fn get(address: &str, path: &str) -> String {
    use std::io::{Read as _, Write as _};
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nhost: {address}\r\nconnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

/// With `--upstream http://h/mcp`, the proxy's root is `/mcp` itself, a redirect
/// into the upstream comes back through the proxy, and a client pointed at the
/// wrong path — `/mcp` again, so `/mcp/mcp` upstream — hears about the 404s at
/// exit instead of nothing at all. The startup lines name the client URL.
#[test]
fn the_client_url_redirects_and_404s_are_spelled_out() {
    let server = upstream(|own, path| match path {
        "/mcp" => format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{own}/mcp/next\r\n\
             content-length: 0\r\nconnection: close\r\n\r\n"
        ),
        _ => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
    });
    let trace = scratch("paths.jsonl");
    let (mut proxy, address, lines) = Proxy::start(http(&trace, &format!("http://{server}/mcp")));
    let redirect = get(&address, "/");
    assert!(redirect.starts_with("HTTP/1.1 307"), "{redirect}");
    assert!(
        redirect
            .to_ascii_lowercase()
            .contains(&format!("location: http://{address}/next\r\n")),
        "{redirect}"
    );
    let missing = get(&address, "/mcp");
    assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
    assert_eq!(proxy.interrupt().code(), Some(0));
    std::thread::sleep(Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    let has = |text: &str| stderr.iter().any(|line| line.contains(text));
    assert!(
        has(&format!(
            "client URL: http://{address}/ (it reaches http://{server}/mcp;"
        )),
        "{stderr:?}"
    );
    assert!(has("2 response(s) had a status outside 2xx"), "{stderr:?}");
    assert!(
        has("the server answered 404 to 1 request(s) — is the endpoint path right?"),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();

    // A root upstream: the client URL needs the endpoint path, and with no
    // traffic there is nothing to remark on.
    let trace = scratch("root.jsonl");
    let (mut proxy, address, lines) = Proxy::start(http(&trace, &format!("http://{server}")));
    assert_eq!(proxy.interrupt().code(), Some(0));
    std::thread::sleep(Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    let has = |text: &str| stderr.iter().any(|line| line.contains(text));
    assert!(has(&format!("e.g. http://{address}/mcp")), "{stderr:?}");
    assert!(!has("outside 2xx"), "{stderr:?}");
    assert!(!has("404"), "{stderr:?}");
    assert!(!has("cannot hold"), "{stderr:?}");
    assert!(!has("other than `message`"), "{stderr:?}");
    std::fs::remove_file(&trace).ok();
}

/// What the proxy forwarded but left out of the trace rather than record wrongly
/// is named at exit: JSON the trace cannot hold as sent, and SSE events of a type
/// MCP clients do not read as messages.
#[test]
fn json_the_trace_cannot_hold_and_typed_events_are_reported() {
    let server = upstream(|_, _| {
        let body = "event: ping\ndata: {}\n\ndata: {\"id\":1,\"id\":2}\n\n";
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\
             connection: close\r\n\r\n{body}",
            body.len()
        )
    });
    let trace = scratch("unrecorded.jsonl");
    let (mut proxy, address, lines) = Proxy::start(http(&trace, &format!("http://{server}")));
    let response = get(&address, "/mcp");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(proxy.interrupt().code(), Some(0));
    std::thread::sleep(Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    let has = |text: &str| stderr.iter().any(|line| line.contains(text));
    assert!(
        has("1 session message(s) were JSON the trace cannot hold as sent"),
        "{stderr:?}"
    );
    assert!(
        has("1 SSE event(s) had a type other than `message`"),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();
}
