// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The command line: what it accepts. `--upstream` and `--listen` are checked
//! by [`mcp_trace_capture::run::values`], which explains a value it refuses
//! without repeating it.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use mcp_trace_capture::DEFAULT_MAX_MESSAGE;
use mcp_trace_capture::run::HostHeader;
use mcp_trace_capture::run::values::listen_address;

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
pub(crate) struct Cli {
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
    pub(crate) output: PathBuf,
    /// Overwrite the output file if it exists (not needed with `{session}`).
    #[arg(long, global = true)]
    pub(crate) force: bool,
    /// The largest message recorded, in bytes; larger ones are forwarded unrecorded.
    #[arg(long, value_name = "BYTES", default_value_t = DEFAULT_MAX_MESSAGE, global = true)]
    pub(crate) max_message_bytes: usize,
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
        /// upstream, so the server's own `Host` validation sees the client's: the
        /// default for an upstream on this machine (`localhost`, `127.0.0.0/8`,
        /// `::1`), not for any other, which may route on its name.
        #[arg(long, conflicts_with = "no_preserve_host")]
        preserve_host: bool,
        /// Name the upstream in `Host` even for an upstream on this machine (the
        /// default for any other); a server's `Host`-based DNS-rebinding check
        /// then passes for every request the proxy forwards.
        #[arg(long)]
        no_preserve_host: bool,
    },
}

impl Mode {
    /// What the server sees as `Host`, from the two flags (clap refuses both
    /// at once).
    pub(crate) const fn host_header(preserve: bool, no_preserve: bool) -> HostHeader {
        match (preserve, no_preserve) {
            (true, _) => HostHeader::Client,
            (false, true) => HostHeader::Upstream,
            (false, false) => HostHeader::Auto,
        }
    }
}
