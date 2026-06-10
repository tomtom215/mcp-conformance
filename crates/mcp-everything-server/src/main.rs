// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-everything-server` — serve the reference server over a chosen transport.
//!
//! Exit codes: `0` clean shutdown, `1` serve/transport failure, `2` invocation
//! error (clap's convention).
//!
//! Stdout discipline: over stdio, **stdout belongs to the protocol**. Nothing
//! in this binary writes diagnostics to stdout; failures report to stderr.
//! Over HTTP, startup prints one `listening on <addr>` line to stderr so
//! orchestration (the conformance runner) can wait for readiness.

use std::net::SocketAddr;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use mcp_everything_server::EverythingServer;
use mcp_everything_server::policy::HttpSecurityPolicy;
use rmcp::ServiceExt as _;
use rmcp::transport::stdio;

/// Reference MCP server for conformance testing.
#[derive(Debug, Parser)]
#[command(name = "mcp-everything-server", version, about, long_about = None)]
struct Cli {
    /// Transport to serve on.
    #[arg(long, value_enum, default_value_t = Transport::Stdio)]
    transport: Transport,
    /// Bind address for the HTTP transport.
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: SocketAddr,
    /// Additional allowed `Host`/`Origin` hostnames (repeatable); replaces
    /// the loopback-only default allowlist.
    #[arg(long = "allowed-host")]
    allowed_hosts: Vec<String>,
    /// Disable Host/Origin validation entirely. This reopens the DNS
    /// rebinding class the default closes — acceptable only behind a
    /// reverse proxy that already enforces host policy.
    #[arg(long)]
    dangerously_allow_any_host: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    /// JSON-RPC over stdin/stdout (subprocess wiring).
    Stdio,
    /// Streamable HTTP on `--bind`, policy-gated (403 on bad Host/Origin).
    Http,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let policy = if cli.dangerously_allow_any_host {
        HttpSecurityPolicy::default().dangerously_allow_any_host()
    } else if cli.allowed_hosts.is_empty() {
        HttpSecurityPolicy::default()
    } else {
        HttpSecurityPolicy::with_allowed_hosts(cli.allowed_hosts.clone())
    };
    match cli.transport {
        Transport::Stdio => serve_stdio().await,
        Transport::Http => serve_http(cli.bind, policy).await,
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

async fn serve_http(bind: SocketAddr, policy: HttpSecurityPolicy) -> ExitCode {
    let app = mcp_everything_server::http::router(policy);
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("mcp-everything-server: cannot bind {bind}: {error}");
            return ExitCode::FAILURE;
        }
    };
    match listener.local_addr() {
        Ok(addr) => eprintln!("listening on {addr}"),
        Err(error) => {
            eprintln!("mcp-everything-server: no local addr: {error}");
            return ExitCode::FAILURE;
        }
    }
    match axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mcp-everything-server: serve error: {error}");
            ExitCode::FAILURE
        }
    }
}
