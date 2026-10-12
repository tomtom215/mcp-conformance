// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The command line: what it accepts, and how a value it refuses is explained.
//!
//! `--upstream` is taken as text and parsed here, not by clap: clap's error
//! repeats the value, and an upstream URL can carry a password or an API key
//! (`http://user:pa ss@host/`, `?key=…`). Errors about it say what is wrong
//! without the value.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::Uri;
use clap::{Parser, Subcommand};
use mcp_trace_capture::DEFAULT_MAX_MESSAGE;

/// The exit codes, for `--help`; the README's table says the same.
const EXIT_CODES: &str = "\
Exit status:
  stdio  the server's exit code (128 + signal if a signal ended it, on Unix;
         the whole code on Windows); 1 if waiting for the server failed
  http   0 after a stop; 128 + signal when a second signal cut the stop short;
         2 if serving failed
  2      bad arguments, or a run that could not start (no trace file is created)
  3      a trace is incomplete (a write failed), in place of a 0";

/// Record a Model Context Protocol session as a trace for `mcp-trace-validator`.
#[derive(Debug, Parser)]
#[command(name = "mcp-trace-capture", version, about, long_about = None, after_help = EXIT_CODES)]
pub struct Cli {
    /// Where to write the trace (JSON Lines); `{session}` in the file name numbers a
    /// trace per session.
    ///
    /// With `{session}` in the file name, each session gets its own numbered file
    /// (`traces/{session}.jsonl` writes `traces/001.jsonl`, `002`, …), and no
    /// existing file is ever overwritten.
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "mcp-trace.jsonl",
        global = true
    )]
    pub output: PathBuf,
    /// Overwrite the output file if it exists (not needed with `{session}`).
    #[arg(long, global = true)]
    pub force: bool,
    /// The largest message recorded, in bytes; larger ones are forwarded unrecorded.
    #[arg(long, value_name = "BYTES", default_value_t = DEFAULT_MAX_MESSAGE, global = true)]
    pub max_message_bytes: usize,
    #[command(subcommand)]
    pub(crate) mode: Mode,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Mode {
    /// Wrap a stdio server: configure your client to launch
    /// `mcp-trace-capture stdio -- <server> [args…]` instead of the server.
    #[command(after_help = EXIT_CODES)]
    Stdio {
        /// The server command and its arguments.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
    /// Proxy a streamable-HTTP server: point your client at the listen address.
    #[command(after_help = EXIT_CODES)]
    Http {
        /// The server's URL. The proxy's `/` is this URL itself; any other request
        /// path is appended to its path.
        #[arg(long, value_name = "URL")]
        upstream: String,
        /// Where the proxy listens: an IP address and a port.
        #[arg(long, value_name = "IP:PORT", default_value = "127.0.0.1:8080", value_parser = listen_address)]
        listen: SocketAddr,
        /// Forward the client's `Host` header unchanged instead of naming the
        /// upstream, so the server's own `Host` validation sees the client's.
        #[arg(long)]
        preserve_host: bool,
    },
}

/// Parses `--upstream` without ever repeating it.
///
/// # Errors
///
/// What is wrong with it, for the user — naming at most the host and path of a
/// URL that lacks only its scheme, never a password or a query.
pub fn upstream(text: &str) -> Result<Uri, String> {
    let authority = text
        .split_once("://")
        .map_or(text, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.contains('@') {
        return Err(
            "the URL carries credentials (user:password@), which the proxy would not send; \
             have the client send them instead, e.g. as an Authorization header"
                .to_owned(),
        );
    }
    if !text.contains("://")
        && let Ok(guess) = format!("http://{text}").parse::<Uri>()
        && guess.host().is_some()
    {
        return Err(format!(
            "the URL has no scheme; did you mean http://{}{}?",
            guess.authority().map_or("", |authority| authority.as_str()),
            guess.path().trim_end_matches('/')
        ));
    }
    text.parse().map_err(|_| {
        "not a valid URL (the value is not repeated, since it can carry credentials); a \
         space or other character a URL cannot hold must be percent-encoded"
            .to_owned()
    })
}

/// Parses `--listen`, explaining the two near misses: a port alone, and a host
/// name where an IP address belongs.
fn listen_address(text: &str) -> Result<SocketAddr, String> {
    if let Ok(address) = text.parse() {
        return Ok(address);
    }
    if text.parse::<u16>().is_ok() {
        return Err(format!(
            "an IP address and a port are needed, e.g. 127.0.0.1:{text}"
        ));
    }
    if let Some((_, port)) = text.rsplit_once(':')
        && port.parse::<u16>().is_ok()
    {
        return Err(format!(
            "the address must be an IP address, not a host name, e.g. 127.0.0.1:{port} or \
             [::1]:{port}"
        ));
    }
    Err("expected IP:PORT, e.g. 127.0.0.1:8080".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn an_upstream_is_explained_without_being_repeated() {
        for secret in [
            "http://user:pa ss@host/",
            "http://user:secret@host/mcp",
            "user:secret@host:1/mcp",
        ] {
            let error = upstream(secret).unwrap_err();
            assert!(error.contains("credentials"), "{error}");
            assert!(
                !error.contains("secret") && !error.contains("pa ss"),
                "{error}"
            );
        }
        let error = upstream("http://h/?key=SEC RET").unwrap_err();
        assert!(!error.contains("SEC"), "{error}");
        assert!(error.contains("not a valid URL"), "{error}");
        // `@` in the query is not credentials.
        assert!(upstream("http://h/mcp?to=a@b").is_ok());
        assert!(upstream("http://h:1/mcp").is_ok());
    }

    #[test]
    fn an_upstream_without_a_scheme_gets_one_suggested() {
        assert_eq!(
            upstream("localhost:3999").unwrap_err(),
            "the URL has no scheme; did you mean http://localhost:3999?"
        );
        assert_eq!(
            upstream("localhost:3999/mcp?key=k").unwrap_err(),
            "the URL has no scheme; did you mean http://localhost:3999/mcp?"
        );
        // A path alone is a URL, if not an absolute one: `Options::new` says so.
        assert!(upstream("/mcp").is_ok());
    }

    #[test]
    fn a_listen_address_near_miss_is_explained() {
        assert_eq!(
            listen_address("127.0.0.1:1").unwrap(),
            "127.0.0.1:1".parse().unwrap()
        );
        assert_eq!(
            listen_address("18081").unwrap_err(),
            "an IP address and a port are needed, e.g. 127.0.0.1:18081"
        );
        assert!(
            listen_address("localhost:18081")
                .unwrap_err()
                .contains("not a host name, e.g. 127.0.0.1:18081 or [::1]:18081")
        );
        assert!(
            listen_address("localhost:port")
                .unwrap_err()
                .starts_with("expected IP:PORT")
        );
        assert!(
            listen_address("")
                .unwrap_err()
                .starts_with("expected IP:PORT")
        );
    }
}
