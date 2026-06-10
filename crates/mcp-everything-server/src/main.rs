// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-everything-server` — serve the reference server over a chosen transport.
//!
//! Exit codes: `0` clean shutdown, `1` serve/transport failure, `2` invocation
//! error (clap's convention).
//!
//! Stdout discipline: over stdio, **stdout belongs to the protocol**. Nothing
//! in this binary writes diagnostics to stdout; failures report to stderr.

use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use mcp_everything_server::EverythingServer;
use rmcp::ServiceExt as _;
use rmcp::transport::stdio;

/// Reference MCP server for conformance testing.
#[derive(Debug, Parser)]
#[command(name = "mcp-everything-server", version, about, long_about = None)]
struct Cli {
    /// Transport to serve on.
    #[arg(long, value_enum, default_value_t = Transport::Stdio)]
    transport: Transport,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    /// JSON-RPC over stdin/stdout (subprocess wiring).
    Stdio,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.transport {
        Transport::Stdio => serve_stdio().await,
    }
}

async fn serve_stdio() -> ExitCode {
    let service = match EverythingServer::new().serve(stdio()).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("mcp-everything-server: failed to start on stdio: {error}");
            return ExitCode::FAILURE;
        }
    };
    match service.waiting().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mcp-everything-server: serve error: {error}");
            ExitCode::FAILURE
        }
    }
}
