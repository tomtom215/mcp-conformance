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
use mcp_everything_server::server::ServedRevision;
use rmcp::ServiceExt as _;
use rmcp::transport::stdio;

/// Reference MCP server for conformance testing.
#[derive(Debug, Parser)]
#[command(name = "mcp-everything-server", version, about, long_about = None)]
struct Cli {
    /// Transport to serve on.
    #[arg(long, value_enum, default_value_t = Transport::Stdio)]
    transport: Transport,
    /// Protocol revision to serve. `2026-07-28` is stateless (SEP-2575): no
    /// `initialize`, no sessions, per-request `_meta` required.
    #[arg(long, value_enum, default_value_t = Revision::default())]
    protocol_version: Revision,
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
    /// Record each HTTP session as a validator-ready JSON Lines trace in
    /// this directory (one file per session). HTTP transport only.
    #[cfg(feature = "tap")]
    #[arg(long, value_name = "DIR")]
    tap_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    /// JSON-RPC over stdin/stdout (subprocess wiring).
    Stdio,
    /// Streamable HTTP on `--bind`, policy-gated (403 on bad Host/Origin).
    Http,
}

/// The CLI spelling of [`ServedRevision`].
///
/// A separate enum rather than `#[derive(ValueEnum)]` on the library type:
/// the library deliberately carries no clap dependency (ADR-0005), and the
/// value names here are the wire identifiers a user would type, not Rust
/// identifiers. [`ServedRevision`] is `#[non_exhaustive]`, so a revision added
/// upstream that this binary has not been taught to name is a compile error
/// here — which is the outcome to want.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum Revision {
    /// Sessions and `initialize`; the surface the conformance registry judges.
    #[default]
    #[value(name = "2025-11-25")]
    V20251125,
    /// Stateless: `server/discover`, per-request `_meta`, caching hints.
    #[value(name = "2026-07-28")]
    V20260728,
}

impl From<Revision> for ServedRevision {
    fn from(revision: Revision) -> Self {
        match revision {
            Revision::V20251125 => Self::V2025_11_25,
            Revision::V20260728 => Self::V2026_07_28,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let revision = ServedRevision::from(cli.protocol_version);
    let policy = if cli.dangerously_allow_any_host {
        HttpSecurityPolicy::default().dangerously_allow_any_host()
    } else if cli.allowed_hosts.is_empty() {
        HttpSecurityPolicy::default()
    } else {
        HttpSecurityPolicy::with_allowed_hosts(cli.allowed_hosts.clone())
    };
    #[cfg(feature = "tap")]
    if cli.tap_dir.is_some() && matches!(cli.transport, Transport::Stdio) {
        eprintln!("mcp-everything-server: --tap-dir requires --transport http");
        return ExitCode::from(2);
    }
    if revision.is_stateless() && matches!(cli.transport, Transport::Stdio) {
        // Refused rather than served badly. rmcp's stdio server is built
        // around the handshake — `serve()` waits for an `initialize` before it
        // dispatches anything — so this combination would answer the refusal
        // this revision owes a legacy client and then exit, which is not a
        // server. `serve_directly` would skip the handshake, but the
        // per-request `_meta` enforcement that makes the stateless surface
        // *conformant* lives in rmcp's HTTP layer and reads HTTP headers;
        // reproducing it for stdio would be new protocol logic with no
        // independent client to validate it against (the official suite drives
        // servers over `--url` only). An unverified conformance claim is the
        // one thing this binary must not ship.
        eprintln!(
            "mcp-everything-server: --protocol-version 2026-07-28 requires --transport http \
             (the stateless surface is not served over stdio here; see the crate README)"
        );
        return ExitCode::from(2);
    }
    match cli.transport {
        Transport::Stdio => serve_stdio(revision).await,
        Transport::Http => {
            #[cfg(feature = "tap")]
            if let Some(dir) = cli.tap_dir {
                return serve_http_tapped(cli.bind, policy, revision, dir).await;
            }
            serve_http(cli.bind, policy, revision).await
        }
    }
}

/// [`serve_http`] with the trace tap installed.
#[cfg(feature = "tap")]
async fn serve_http_tapped(
    bind: SocketAddr,
    policy: HttpSecurityPolicy,
    revision: ServedRevision,
    dir: std::path::PathBuf,
) -> ExitCode {
    let tap = match mcp_everything_server::tap::Tap::new(dir) {
        Ok(tap) => tap,
        Err(error) => {
            eprintln!("mcp-everything-server: cannot create tap directory: {error}");
            return ExitCode::FAILURE;
        }
    };
    let app = mcp_everything_server::http::router_tapped(policy, revision, tap);
    serve_app(bind, app).await
}

async fn serve_stdio(revision: ServedRevision) -> ExitCode {
    let service = match EverythingServer::serving(revision).serve(stdio()).await {
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

async fn serve_http(
    bind: SocketAddr,
    policy: HttpSecurityPolicy,
    revision: ServedRevision,
) -> ExitCode {
    let app = mcp_everything_server::http::router(policy, revision);
    serve_app(bind, app).await
}

/// Binds, prints the readiness line, and serves `app` until ctrl-c.
async fn serve_app(bind: SocketAddr, app: axum::Router) -> ExitCode {
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("mcp-everything-server: cannot bind {bind}: {error}");
            return ExitCode::FAILURE;
        }
    };
    match listener.local_addr() {
        Ok(addr) => eprintln!("{}{addr}", mcp_everything_server::READINESS_LINE_PREFIX),
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
