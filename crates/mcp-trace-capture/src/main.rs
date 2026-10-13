// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-trace-capture` — record an MCP session as a validator-ready trace.
//!
//! Exit codes (stable interface):
//!
//! | Code | Meaning |
//! |------|---------|
//! | server's | `stdio`: the wrapped server's own exit code, passed through (128 + signal when it was killed by one, on Unix; the whole code on Windows) |
//! | 1    | `stdio`: waiting for the server failed |
//! | 0    | `http`: stopped with a complete trace |
//! | 128 + signal | `http`: a second signal stopped the proxy without waiting for open requests |
//! | 2    | Invocation problem: bad arguments, an existing output file, a misplaced `{session}` or a missing directory for it, a server that would not start, an address that would not bind (no trace file is created); also `http` serving failed after it started |
//! | 3    | The session ran but a trace is incomplete (a write failed); only when the exit code would otherwise be 0 |

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::process::ExitCode;

use args::{Cli, Mode};
use clap::Parser as _;
use mcp_trace_capture::output;
use mcp_trace_capture::run::{self, EXIT_USAGE, Recording};

mod args;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let target = match output::check(&cli.output, cli.force) {
        Ok(target) => target,
        Err(message) => {
            eprintln!("mcp-trace-capture: {message}");
            return run::exit(EXIT_USAGE);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("mcp-trace-capture: cannot start the async runtime: {error}");
            return run::exit(EXIT_USAGE);
        }
    };
    let mut recording = Recording::new(&target);
    recording.force = cli.force;
    recording.max_message = cli.max_message_bytes;
    let code = match &cli.mode {
        Mode::Stdio { command } => runtime.block_on(run::stdio(recording, command)),
        Mode::Http {
            upstream,
            listen,
            preserve_host,
        } => runtime.block_on(run::http(recording, upstream, *listen, *preserve_host)),
    };
    // A client blocked on this process's stdin would otherwise hold the runtime open.
    runtime.shutdown_background();
    run::exit(code)
}
