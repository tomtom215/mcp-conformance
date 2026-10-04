// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Each failure an operator meets first, rendered: the cause found by type
//! through the error chain rmcp builds, and the sentence it becomes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use rmcp::transport::DynamicTransportError;

fn url() -> Target {
    Target::Url("http://127.0.0.1:1/mcp".to_owned())
}

fn command() -> Target {
    Target::Command("./server --stdio".to_owned())
}

/// `inner`, wrapped the way rmcp wraps a transport's send failure.
fn transport_error(inner: Box<dyn Error + Send + Sync>) -> ClientInitializeError {
    ClientInitializeError::TransportError {
        error: DynamicTransportError::from_parts(
            "rmcp::transport::worker::WorkerTransport<…>",
            std::any::TypeId::of::<()>(),
            inner,
        ),
        context: "send initialize request".into(),
    }
}

/// The full message for `error` against `target`, asserting no Rust type
/// name survived into it.
fn message(target: &Target, error: &ClientInitializeError) -> String {
    let line = initialization_failure(target, error);
    for leak in [
        "rmcp::",
        "reqwest::",
        "Transport [",
        "WorkerTransport",
        "os error",
    ] {
        assert!(!line.contains(leak), "{leak} leaked into: {line}");
    }
    line
}

#[test]
fn a_refused_connection_names_the_url_and_the_refusal() {
    let error = transport_error(Box::new(std::io::Error::from(
        std::io::ErrorKind::ConnectionRefused,
    )));
    assert_eq!(cause_of(&error), Cause::Refused);
    assert_eq!(
        message(&url(), &error),
        "cannot connect to http://127.0.0.1:1/mcp: connection refused — is the server \
         running, and listening on that port?"
    );
}

/// A real reqwest failure: `url` posted to by a client with `timeout`.
#[cfg(feature = "http")]
async fn reqwest_failure(url: &str, timeout: std::time::Duration) -> reqwest::Error {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .build()
        .unwrap()
        .post(url)
        .send()
        .await
        .expect_err("the request must fail")
}

#[cfg(feature = "http")]
#[tokio::test]
async fn real_reqwest_failures_are_found_through_the_chain() {
    use rmcp::transport::streamable_http_client::StreamableHttpError;
    use std::time::Duration;
    // A port nothing listens on, so the refusal is the OS's own.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = format!("http://{}/mcp", listener.local_addr().unwrap());
    drop(listener);
    let long = Duration::from_secs(10);

    // Wrapped exactly as rmcp's HTTP transport wraps it: the `Client`
    // variant declares no source, so only the by-type unwrapping sees under it.
    let wrapped = reqwest_failure(&closed, long).await;
    let error = transport_error(Box::new(StreamableHttpError::Client(wrapped)));
    assert_eq!(cause_of(&error), Cause::Refused);
    // And bare, should a transport ever hand one over directly.
    let bare = transport_error(Box::new(reqwest_failure(&closed, long).await));
    assert_eq!(cause_of(&bare), Cause::Refused);

    // A listener that accepts and never answers: the client's own timeout.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let silent_url = format!("http://{}/mcp", silent.local_addr().unwrap());
    let timed_out = reqwest_failure(&silent_url, Duration::from_millis(100)).await;
    let error = transport_error(Box::new(StreamableHttpError::Client(timed_out)));
    assert_eq!(cause_of(&error), Cause::TimedOut);
    drop(silent);

    // A request that never left: no I/O, no timeout, its own words.
    let unbuildable = reqwest_failure("not a url", long).await;
    let error = transport_error(Box::new(StreamableHttpError::Client(unbuildable)));
    assert!(matches!(cause_of(&error), Cause::Other(_)), "{error}");
}

#[cfg(feature = "http")]
#[test]
fn a_transport_io_error_is_found_by_the_generic_walk() {
    use rmcp::transport::streamable_http_client::StreamableHttpError;
    let error = transport_error(Box::new(StreamableHttpError::<reqwest::Error>::Io(
        std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
    )));
    assert_eq!(cause_of(&error), Cause::Refused);
}

#[test]
fn io_kinds_map_to_what_they_mean_for_a_connection() {
    for (kind, cause) in [
        (std::io::ErrorKind::TimedOut, Cause::TimedOut),
        (std::io::ErrorKind::UnexpectedEof, Cause::Closed),
        (std::io::ErrorKind::BrokenPipe, Cause::Closed),
        (std::io::ErrorKind::ConnectionRefused, Cause::Refused),
    ] {
        let error = transport_error(Box::new(std::io::Error::from(kind)));
        assert_eq!(cause_of(&error), cause, "{kind:?}");
    }
    let other = transport_error(Box::new(std::io::Error::other("network is down")));
    assert_eq!(cause_of(&other), Cause::Io("network is down".to_owned()));
}

#[test]
fn other_initialization_failures_keep_their_own_words() {
    let refused = ClientInitializeError::JsonRpcError(rmcp::model::ErrorData::invalid_request(
        "unsupported",
        None,
    ));
    assert_eq!(
        message(&url(), &refused),
        "cannot start a session with http://127.0.0.1:1/mcp: the server refused it: \
         unsupported (JSON-RPC error -32600)"
    );
    let unconfigured = ClientInitializeError::NoPreferredProtocolVersion;
    assert_eq!(
        cause_of(&unconfigured),
        Cause::Other(unconfigured.to_string())
    );
}

#[cfg(feature = "http")]
#[test]
fn a_404_says_so_and_asks_about_the_path() {
    use rmcp::transport::streamable_http_client::StreamableHttpError;
    let error = transport_error(Box::new(
        StreamableHttpError::<reqwest::Error>::UnexpectedServerResponse(
            "HTTP 404 Not Found: ".into(),
        ),
    ));
    assert_eq!(
        cause_of(&error),
        Cause::Status(404, "404 Not Found".to_owned())
    );
    let wrong = Target::Url("http://127.0.0.1:8080/wrong".to_owned());
    assert_eq!(
        message(&wrong, &error),
        "http://127.0.0.1:8080/wrong: the server answered 404 Not Found — is the MCP \
         endpoint path right?"
    );
}

#[test]
fn a_refused_stateless_discover_reads_the_same_as_a_404() {
    // At 2026-07-28 rmcp reports the same 404 as a JSON-RPC error instead.
    let error = ClientInitializeError::JsonRpcError(rmcp::model::ErrorData::invalid_request(
        "server/discover rejected with HTTP 404 Not Found: ",
        None,
    ));
    assert_eq!(
        message(&url(), &error),
        "http://127.0.0.1:1/mcp: the server answered 404 Not Found — is the MCP endpoint \
         path right?"
    );
}

#[test]
fn a_stdio_server_that_exits_is_named_and_the_next_step_given() {
    // `false`: the write of `initialize` breaks the pipe.
    let broken = transport_error(Box::new(std::io::Error::from(
        std::io::ErrorKind::BrokenPipe,
    )));
    // `true`: the read of the answer finds the stream already over.
    let closed = ClientInitializeError::ConnectionClosed("initialize response".to_owned());
    for error in [broken, closed] {
        assert_eq!(cause_of(&error), Cause::Closed);
        assert_eq!(
            message(&command(), &error),
            "the server command \"./server --stdio\" exited before the session started — \
             run it by hand to see why"
        );
    }
}

#[test]
fn every_cause_renders_for_both_targets_without_a_debug_dump() {
    let causes = [
        Cause::Refused,
        Cause::TimedOut,
        Cause::Status(500, "500 Internal Server Error".to_owned()),
        Cause::Closed,
        Cause::Io("network unreachable".to_owned()),
        Cause::Other("something odd".to_owned()),
    ];
    for target in [url(), command()] {
        for cause in &causes {
            let line = describe(&target, cause);
            assert!(line.contains(&target.to_string()), "{line}");
            assert!(!line.contains("Cause::"), "{line}");
        }
    }
    assert_eq!(
        describe(&url(), &Cause::TimedOut),
        "cannot connect to http://127.0.0.1:1/mcp: timed out"
    );
    assert_eq!(
        describe(&url(), &Cause::Closed),
        "http://127.0.0.1:1/mcp: the server closed the connection before the session started"
    );
    assert_eq!(
        describe(&url(), &Cause::Status(401, "401 Unauthorized".to_owned())),
        "http://127.0.0.1:1/mcp: the server answered 401 Unauthorized — the server refused \
         this client; it may need authorization"
    );
    assert_eq!(
        describe(
            &url(),
            &Cause::Status(405, "405 Method Not Allowed".to_owned())
        ),
        "http://127.0.0.1:1/mcp: the server answered 405 Method Not Allowed — that URL does \
         not take MCP's POST; is it the MCP endpoint?"
    );
    assert_eq!(
        describe(&command(), &Cause::Refused),
        "the server command \"./server --stdio\" failed before the session started: \
         connection refused"
    );
    assert_eq!(
        describe(&command(), &Cause::TimedOut),
        "the server command \"./server --stdio\" failed before the session started: \
         timed out"
    );
    assert_eq!(
        describe(&command(), &Cause::Io("bad descriptor".to_owned())),
        "the server command \"./server --stdio\" failed before the session started: bad \
         descriptor"
    );
}

#[test]
fn an_unrecognised_error_falls_back_to_its_innermost_words() {
    let error = transport_error(Box::new(std::fmt::Error));
    assert_eq!(
        message(&url(), &error),
        "cannot start a session with http://127.0.0.1:1/mcp: an error occurred when \
         formatting an argument"
    );
}

#[test]
fn a_command_that_cannot_spawn_says_which_program_and_why() {
    let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
    assert_eq!(
        spawn_failure("/no/such/server --stdio", &missing),
        "cannot start the server command \"/no/such/server --stdio\": /no/such/server was \
         not found — check the path, or that it is on PATH"
    );
    let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert!(
        spawn_failure("./server", &denied).contains("./server is not executable"),
        "{}",
        spawn_failure("./server", &denied)
    );
    let busy = std::io::Error::other("text file busy");
    assert_eq!(
        spawn_failure("./server", &busy),
        "cannot start the server command \"./server\": text file busy"
    );
    let empty = std::io::Error::from(std::io::ErrorKind::InvalidInput);
    assert_eq!(
        spawn_failure("  ", &empty),
        "--server-cmd is empty: name the server program to spawn"
    );
}

#[test]
fn the_status_is_read_back_out_of_rmcp_text_and_nothing_else() {
    assert_eq!(
        http_status_in("HTTP 404 Not Found: <html>"),
        Some((404, "404 Not Found".to_owned()))
    );
    assert_eq!(
        http_status_in("server/discover rejected with HTTP 503 Service Unavailable: busy"),
        Some((503, "503 Service Unavailable".to_owned()))
    );
    assert_eq!(http_status_in("HTTP 418"), Some((418, "418".to_owned())));
    for not_a_status in [
        "no status here",
        "HTTP abc",
        "HTTP 4040 Huh",
        "HTTP 999 Nope",
    ] {
        assert_eq!(http_status_in(not_a_status), None, "{not_a_status}");
    }
}
