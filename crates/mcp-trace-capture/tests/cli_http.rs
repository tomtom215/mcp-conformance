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

/// An upstream answering every request with `respond(own address, request
/// head)`; returns its address.
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
            let _ = stream.write_all(respond(&own, &head).as_bytes());
        }
    });
    address
}

/// The request target of the request `head`.
fn path_of(head: &str) -> &str {
    head.split(' ').nth(1).unwrap_or_default()
}

/// The value of the header `name` (lowercase) in the request `head`, if any.
fn header_of<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (field, value) = line.split_once(':')?;
        field.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// Sends `GET path` through the proxy at `address`; returns the response.
fn get(address: &str, path: &str) -> String {
    get_as(address, path, address)
}

/// [`get`], with `host` as the request's `Host`.
fn get_as(address: &str, path: &str, host: &str) -> String {
    use std::io::{Read as _, Write as _};
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

/// A 200 whose body is `text`, on a connection the upstream then closes.
fn ok(text: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    )
}

/// The upstream is on this machine, so the proxy passes the client's `Host`
/// through by default: a server's DNS-rebinding check then judges what the
/// client sent, where the proxy's rewrite to the server's own name made that
/// check pass for every request. `--no-preserve-host` restores the rewrite,
/// `--preserve-host` asks for the default in so many words, and the startup
/// lines say which is in force.
#[test]
fn a_loopback_upstream_sees_the_clients_host_unless_told_otherwise() {
    let server = upstream(|_, head| ok(&format!("host={}", header_of(head, "host").unwrap_or(""))));
    let cases = [
        (None, "evil.example", "client's Host"),
        (Some("--no-preserve-host"), server.as_str(), "own name"),
        (Some("--preserve-host"), "evil.example", "client's Host"),
    ];
    for (flag, expected_host, announced) in cases {
        let trace = scratch(&format!("host-{}.jsonl", flag.unwrap_or("default")));
        let mut command = http(&trace, &format!("http://{server}/mcp"));
        if let Some(flag) = flag {
            command.arg(flag);
        }
        let (mut proxy, address, lines) = Proxy::start(command);
        let response = get_as(&address, "/", "evil.example");
        assert!(
            response.ends_with(&format!("host={expected_host}")),
            "{flag:?}: {response}"
        );
        assert_eq!(proxy.interrupt().code(), Some(0));
        std::thread::sleep(Duration::from_millis(100));
        let stderr: Vec<String> = lines.try_iter().collect();
        assert!(
            stderr
                .iter()
                .any(|line| line.contains("Host") && line.contains(announced)),
            "{flag:?}: {stderr:?}"
        );
        std::fs::remove_file(&trace).ok();
    }
}

/// With `--upstream http://h/mcp`, the proxy's root is `/mcp` itself, a redirect
/// into the upstream comes back through the proxy, and a client pointed at the
/// wrong path — `/mcp` again, so `/mcp/mcp` upstream — hears about the 404s at
/// exit instead of nothing at all. The startup lines name the client URL.
#[test]
fn the_client_url_redirects_and_404s_are_spelled_out() {
    let server = upstream(|own, head| match path_of(head) {
        "/mcp" => format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{own}/mcp/next\r\n\
             content-length: 0\r\nconnection: close\r\n\r\n"
        ),
        "/ok" => "HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
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

    // A root upstream: the client URL needs the endpoint path, and with only
    // a 200 answered there is nothing to remark on.
    let trace = scratch("root.jsonl");
    let (mut proxy, address, lines) = Proxy::start(http(&trace, &format!("http://{server}")));
    let ok = get(&address, "/ok");
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    assert_eq!(proxy.interrupt().code(), Some(0));
    std::thread::sleep(Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    let has = |text: &str| stderr.iter().any(|line| line.contains(text));
    assert!(has(&format!("e.g. http://{address}/mcp")), "{stderr:?}");
    assert!(!has("outside 2xx"), "{stderr:?}");
    // The 404 remark, not the digits: the trace path carries the process id.
    assert!(!has("answered 404"), "{stderr:?}");
    assert!(!has("cannot hold"), "{stderr:?}");
    assert!(!has("other than `message`"), "{stderr:?}");
    std::fs::remove_file(&trace).ok();
}

/// A response with `status`, a `content-type` and `body`, on a connection the
/// upstream then closes.
fn answer(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n{body}",
        body.len()
    )
}

/// An upstream whose paths answer 404 in four ways: `/rpc` with a JSON-RPC
/// error (`ping`'s method-not-found), `/framework` with a web framework's own
/// JSON, `/html` with a page, `/empty` with nothing; any other path is a 200.
fn upstream_answering_404s() -> String {
    upstream(|_, head| match path_of(head) {
        "/rpc" => answer(
            "404 Not Found",
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found","data":"ping"}}"#,
        ),
        "/framework" => answer(
            "404 Not Found",
            "application/json",
            r#"{"detail":"Not Found"}"#,
        ),
        "/html" => answer(
            "404 Not Found",
            "text/html",
            "<h1>Cannot POST /mcp/mcp</h1>",
        ),
        "/empty" => {
            "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned()
        }
        _ => ok(""),
    })
}

/// The proxy's stderr after a run in front of `server` (a root upstream) that
/// got each of `paths` and was answered with `status`.
fn stderr_after_gets(server: &str, paths: &[&str], status: &str) -> Vec<String> {
    let trace = scratch("hints.jsonl");
    let (mut proxy, address, lines) = Proxy::start(http(&trace, &format!("http://{server}")));
    for path in paths {
        let response = get(&address, path);
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{path}: {response}"
        );
    }
    assert_eq!(proxy.interrupt().code(), Some(0));
    std::fs::remove_file(&trace).ok();
    drained(&lines)
}

/// Every stderr line the proxy wrote: read until its stderr closes, which it
/// does when the process has exited, so a loaded machine cannot cut it short.
fn drained(lines: &Receiver<String>) -> Vec<String> {
    let mut all = Vec::new();
    while let Ok(line) = lines.recv_timeout(Duration::from_secs(20)) {
        all.push(line);
    }
    all
}

/// The path hint is for a client pointed at the wrong path. A 404 that carries a
/// JSON-RPC message is the MCP server speaking — the specification has a server
/// answer `ping`'s method-not-found with `404`, and a session it ended with
/// `404` — so it counts as a status outside 2xx and draws no remark about the
/// path (the dogfood audit's finding 3.3). One that does not (HTML, a framework's
/// JSON of its own, nothing) still does.
#[test]
fn a_404_the_mcp_server_sent_is_not_a_wrong_path() {
    let server = upstream_answering_404s();
    let run = |paths: &[&str]| stderr_after_gets(&server, paths, "404");
    let has = |stderr: &[String], text: &str| stderr.iter().any(|line| line.contains(text));

    let spoke = run(&["/rpc"]);
    assert!(
        has(&spoke, "1 response(s) had a status outside 2xx"),
        "{spoke:?}"
    );
    assert!(!has(&spoke, "answered 404"), "{spoke:?}");

    for path in ["/framework", "/html", "/empty"] {
        let wrong = run(&[path]);
        assert!(
            has(
                &wrong,
                "the server answered 404 to 1 request(s) — is the endpoint path right?"
            ),
            "{path}: {wrong:?}"
        );
    }

    // Counted by what each was: of four 404s, the one that spoke MCP is not.
    let mixed = run(&["/rpc", "/html", "/framework", "/rpc", "/empty"]);
    assert!(
        has(&mixed, "5 response(s) had a status outside 2xx"),
        "{mixed:?}"
    );
    assert!(
        has(
            &mixed,
            "the server answered 404 to 3 request(s) — is the endpoint path right?"
        ),
        "{mixed:?}"
    );
}

/// A server's `Host`/`Origin` check refuses with `403`. With the client's `Host`
/// passed through — the default for a server on this machine — a client that
/// reached the proxy by a name the server does not allow gets that refusal, and
/// the proxy is the one place that can say how to see the server without it
/// (the audit's missing 403 hint): `--no-preserve-host`. It says so only when
/// it could be the cause: the `Host` was the client's, and named a host other
/// than the server's.
#[test]
fn a_403_to_a_forwarded_foreign_host_names_the_flag_that_stops_forwarding_it() {
    let server = upstream(|_, head| match path_of(head) {
        "/mcp/refused" => answer(
            "403 Forbidden",
            "application/json",
            r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Forbidden: Host or Origin header rejected by security policy"}}"#,
        ),
        _ => ok(""),
    });
    let server_name = server.split(':').next().unwrap().to_owned();
    let run = |flag: Option<&str>, host: &str| -> Vec<String> {
        let trace = scratch("hint403.jsonl");
        let mut command = http(&trace, &format!("http://{server}/mcp"));
        if let Some(flag) = flag {
            command.arg(flag);
        }
        let (mut proxy, address, lines) = Proxy::start(command);
        let response = get_as(&address, "/refused", host);
        assert!(
            response.starts_with("HTTP/1.1 403"),
            "{flag:?} {host}: {response}"
        );
        assert_eq!(proxy.interrupt().code(), Some(0));
        std::fs::remove_file(&trace).ok();
        drained(&lines)
    };
    let has = |stderr: &[String], text: &str| stderr.iter().any(|line| line.contains(text));
    let hint = "the server answered 403 to 1 request(s) whose Host header (a name other than the \
                server's own) was passed through to it; if that is its Host/Origin check, \
                --no-preserve-host sends the server's own name instead";

    // The default for a server on this machine, and the flag that asks for it.
    for flag in [None, Some("--preserve-host")] {
        let stderr = run(flag, "box.internal:8080");
        assert!(
            has(&stderr, "1 response(s) had a status outside 2xx"),
            "{stderr:?}"
        );
        assert!(has(&stderr, hint), "{flag:?}: {stderr:?}");
    }
    // The server's own name was sent: its Host check cannot be what refused.
    let stderr = run(Some("--no-preserve-host"), "box.internal:8080");
    assert!(
        has(&stderr, "1 response(s) had a status outside 2xx"),
        "{stderr:?}"
    );
    assert!(!has(&stderr, "answered 403"), "{stderr:?}");
    // The client's Host names the server's own host (only the port differs):
    // not a name the server's check could object to, so no remark.
    let stderr = run(None, &format!("{server_name}:8080"));
    assert!(!has(&stderr, "answered 403"), "{stderr:?}");
}

/// What the proxy forwarded but left out of the trace rather than record wrongly
/// is named at exit: JSON the trace cannot hold as sent, and SSE events of a type
/// MCP clients do not read as messages.
#[test]
fn json_the_trace_cannot_hold_and_typed_events_are_reported() {
    let server = upstream(|_, _head| {
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
