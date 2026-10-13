// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-conformance` — the official suite through a recording tap, and both
//! verdicts on the same traffic (`test`); a session judged as it runs
//! (`watch`).
//!
//! `watch` exits as `mcp-trace-capture` does (a stdio client relies on the
//! server's exit code passing through). `test`'s exit codes (stable interface,
//! consistent with `mcp-trace-validator`'s):
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | The suite passed, and the validator agrees with it: no failing clause the divergence baseline leaves unexplained, and no stale baseline entry |
//! | 1    | Findings: a suite check failed or warned, or the two verdicts disagree |
//! | 2    | The run could not be made or judged: bad arguments, Node.js or npx missing, the suite did not run or reported another version, no recorded session could be judged, or the report could not be written |

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::process::ExitCode;

use clap::Parser as _;

mod args;
mod bridge;
mod judging;
mod node;
mod proxy;
mod relay;
mod render;
mod run;
mod watch;

const EXIT_OK: u8 = 0;
const EXIT_FINDINGS: u8 = 1;
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let cli = args::Cli::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("mcp-conformance: cannot start the async runtime: {error}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let code = match cli.command {
        args::Command::Test(args) => ExitCode::from(runtime.block_on(run::test(*args))),
        args::Command::Watch(args) => watch::main(&runtime, &args),
        args::Command::ClientRelay { command } => {
            ExitCode::from(runtime.block_on(relay::run(command)))
        }
    };
    // A client blocked on this process's stdin would otherwise hold the
    // runtime open (`watch stdio`).
    runtime.shutdown_background();
    code
}
