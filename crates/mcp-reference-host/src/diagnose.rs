// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Errors for humans: why a session could not start, in the operator's words.
//!
//! rmcp's errors are written for the code that handles them. Printed as they
//! are, a refused port reads `Send message error Transport
//! [rmcp::transport::worker::WorkerTransport<…reqwest…>] error: Client error:
//! error sending request for url (…)` — the generic type of a transport worker,
//! and no word of what went wrong. This module finds the cause and says it:
//! `cannot connect to http://127.0.0.1:1/mcp: connection refused`.
//!
//! The cause is found by *type*, not by reading `Debug` output: the error's
//! [`source`](std::error::Error::source) chain is walked and each link
//! downcast to the kinds that carry an answer — [`std::io::Error`] for a
//! refused or closed connection, `reqwest`'s error for an HTTP request that
//! never got a response. Two rmcp wrappers name their inner error without
//! declaring it a source, so the chain stops at them; those two are downcast
//! by their concrete type and unwrapped by hand.
//!
//! One fact is only available as text: an HTTP status. rmcp reports a
//! non-success response as a string — `HTTP 404 Not Found: <body>` — so
//! [`http_status_in`] reads the status back out of it. That is parsing a value
//! rmcp formatted from `http::StatusCode`, in the one shape it uses, and it
//! degrades to the original text when the shape changes.

use std::error::Error;
use std::fmt;

use rmcp::service::ClientInitializeError;

/// What the host was trying to reach, for naming it in a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A Streamable HTTP endpoint.
    Url(String),
    /// A stdio server, spawned from this command line.
    Command(String),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(url) => f.write_str(url),
            Self::Command(command) => write!(f, "the server command {command:?}"),
        }
    }
}

/// Why a session could not start, reduced to what an operator can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    /// Nothing listens there.
    Refused,
    /// The connection or request did not finish in time.
    TimedOut,
    /// The server answered, with this non-success status (`404 Not Found`).
    Status(u16, String),
    /// The other end went away before the session started: a stdio server
    /// that exited, or a connection closed without an answer.
    Closed,
    /// An I/O failure with no more specific reading, as the OS put it.
    Io(String),
    /// Anything else, as its innermost error put it.
    Other(String),
}

/// The one-line diagnostic for a session that failed to start.
#[must_use]
pub fn initialization_failure(target: &Target, error: &ClientInitializeError) -> String {
    describe(target, &cause_of(error))
}

/// The one-line diagnostic for a stdio server command that could not spawn.
#[must_use]
pub fn spawn_failure(command: &str, error: &std::io::Error) -> String {
    let program = command.split(' ').find(|part| !part.is_empty());
    match (error.kind(), program) {
        (_, None) => "--server-cmd is empty: name the server program to spawn".to_owned(),
        (std::io::ErrorKind::NotFound, Some(program)) => format!(
            "cannot start the server command {command:?}: {program} was not found — \
             check the path, or that it is on PATH"
        ),
        (std::io::ErrorKind::PermissionDenied, Some(program)) => format!(
            "cannot start the server command {command:?}: {program} is not executable \
             (permission denied)"
        ),
        (_, Some(_)) => format!("cannot start the server command {command:?}: {error}"),
    }
}

/// Renders `cause` for `target`.
#[must_use]
pub fn describe(target: &Target, cause: &Cause) -> String {
    match (target, cause) {
        (Target::Url(url), Cause::Refused) => format!(
            "cannot connect to {url}: connection refused — is the server running, and \
             listening on that port?"
        ),
        (Target::Url(url), Cause::TimedOut) => format!("cannot connect to {url}: timed out"),
        (Target::Url(url), Cause::Io(detail)) => format!("cannot connect to {url}: {detail}"),
        (_, Cause::Status(code, status)) => {
            format!(
                "{target}: the server answered {status}{}",
                status_hint(*code)
            )
        }
        (Target::Url(url), Cause::Closed) => {
            format!("{url}: the server closed the connection before the session started")
        }
        (Target::Command(_), Cause::Closed) => {
            format!("{target} exited before the session started — run it by hand to see why")
        }
        (Target::Command(_), Cause::Io(detail)) => {
            format!("{target} failed before the session started: {detail}")
        }
        // Neither can come from a pipe; said plainly should a transport ever
        // report one anyway.
        (Target::Command(_), Cause::Refused) => {
            format!("{target} failed before the session started: connection refused")
        }
        (Target::Command(_), Cause::TimedOut) => {
            format!("{target} failed before the session started: timed out")
        }
        (_, Cause::Other(detail)) => format!("cannot start a session with {target}: {detail}"),
    }
}

/// What a status most often means for an MCP endpoint, when it means one thing.
const fn status_hint(code: u16) -> &'static str {
    match code {
        404 => " — is the MCP endpoint path right?",
        405 => " — that URL does not take MCP's POST; is it the MCP endpoint?",
        401 | 403 => " — the server refused this client; it may need authorization",
        _ => "",
    }
}

/// The cause behind an initialization failure.
#[must_use]
pub fn cause_of(error: &ClientInitializeError) -> Cause {
    match error {
        ClientInitializeError::TransportError { error, .. } => transport_cause(&*error.error),
        // At `2026-07-28` rmcp turns a refused `server/discover` into a
        // JSON-RPC error carrying the HTTP status in its message.
        ClientInitializeError::JsonRpcError(data) => http_status_in(&data.message).map_or_else(
            || {
                Cause::Other(format!(
                    "the server refused it: {} (JSON-RPC error {})",
                    data.message, data.code.0
                ))
            },
            |(code, status)| Cause::Status(code, status),
        ),
        ClientInitializeError::ConnectionClosed(_) => Cause::Closed,
        other => Cause::Other(other.to_string()),
    }
}

/// The cause inside a transport's error, found by walking its source chain.
#[must_use]
pub fn transport_cause(error: &(dyn Error + 'static)) -> Cause {
    for link in chain(error) {
        #[cfg(feature = "http")]
        if let Some(cause) = http_cause(link) {
            return cause;
        }
        if let Some(io) = link.downcast_ref::<std::io::Error>() {
            return io_cause(io);
        }
    }
    Cause::Other(innermost(error).to_string())
}

/// The Streamable HTTP transport's own error, when `link` is one.
///
/// Its `Client` variant holds the reqwest error without declaring it a
/// source, so the generic walk would stop here and see nothing.
#[cfg(feature = "http")]
fn http_cause(link: &(dyn Error + 'static)) -> Option<Cause> {
    use rmcp::transport::streamable_http_client::StreamableHttpError;
    if let Some(error) = link.downcast_ref::<StreamableHttpError<reqwest::Error>>() {
        return match error {
            StreamableHttpError::Client(request) => Some(request_cause(request)),
            StreamableHttpError::UnexpectedServerResponse(text) => {
                http_status_in(text).map(|(code, status)| Cause::Status(code, status))
            }
            // Everything else either declares its source (`Io` among them),
            // which the generic walk then finds, or has none to find.
            _ => None,
        };
    }
    link.downcast_ref::<reqwest::Error>().map(request_cause)
}

/// The cause of an HTTP request that got no response.
#[cfg(feature = "http")]
fn request_cause(error: &reqwest::Error) -> Cause {
    if let Some(io) = chain(error).find_map(|link| link.downcast_ref::<std::io::Error>()) {
        return io_cause(io);
    }
    if error.is_timeout() {
        return Cause::TimedOut;
    }
    Cause::Other(innermost(error).to_string())
}

/// An I/O error's meaning for a connection.
fn io_cause(io: &std::io::Error) -> Cause {
    use std::io::ErrorKind;
    match io.kind() {
        ErrorKind::ConnectionRefused => Cause::Refused,
        ErrorKind::TimedOut => Cause::TimedOut,
        // A stdio server that exited: writing to it breaks the pipe, reading
        // from it ends early.
        ErrorKind::BrokenPipe | ErrorKind::UnexpectedEof => Cause::Closed,
        _ => Cause::Io(io.to_string()),
    }
}

/// `error` and every error under it, outermost first.
fn chain<'a>(error: &'a (dyn Error + 'static)) -> impl Iterator<Item = &'a (dyn Error + 'static)> {
    std::iter::successors(Some(error), |&link| link.source())
}

/// The last link of `error`'s source chain: usually the one in plain words.
fn innermost<'a>(error: &'a (dyn Error + 'static)) -> &'a (dyn Error + 'static) {
    chain(error).last().unwrap_or(error)
}

/// The status in rmcp's `HTTP <code> <reason>[: <body>]` text, if it has one.
///
/// Returns the numeric code and the status as `http::StatusCode` displays it
/// (`404 Not Found`), without the body.
#[must_use]
pub fn http_status_in(text: &str) -> Option<(u16, String)> {
    let (_, after) = text.split_once("HTTP ")?;
    let status = after.split(':').next().unwrap_or(after).trim();
    let code = status.get(..3)?.parse::<u16>().ok()?;
    let rest = status.get(3..)?;
    if !(100..=599).contains(&code) || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some((code, status.to_owned()))
}

#[cfg(test)]
mod tests;
