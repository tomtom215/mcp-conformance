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
use mcp_everything_server::server::stateless::StatelessEnvelope;
use rmcp::ServiceExt as _;
use rmcp::service::serve_directly;
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
    let server = EverythingServer::serving(revision);
    if revision.is_stateless() {
        return serve_stdio_stateless(server).await;
    }
    let service = match server.serve(stdio()).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("mcp-everything-server: failed to start on stdio: {error}");
            return ExitCode::FAILURE;
        }
    };
    waited(service.waiting().await)
}

/// Serves the stateless surface over stdio.
///
/// `serve` cannot: it waits for an `initialize` before dispatching anything,
/// and this revision removed that message. `serve_directly` is rmcp's entry
/// point for exactly this — a peer with no handshake and no negotiated state —
/// and [`StatelessEnvelope`] supplies the per-request checking that rmcp
/// performs inside its HTTP layer and that stdio therefore has nobody to do.
///
/// There is no readiness line and nothing to bind: over stdio the server is
/// ready when the process is, and stdout belongs to the protocol.
async fn serve_stdio_stateless(server: EverythingServer) -> ExitCode {
    let service = serve_directly(StatelessEnvelope(server), stdio(), None);
    waited(service.waiting().await)
}

/// The exit code for a service that has stopped.
fn waited<T, E: std::fmt::Display>(outcome: Result<T, E>) -> ExitCode {
    match outcome {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_service_reports_which_way_it_stopped() {
        // The one place a serve failure becomes an exit code, and the only
        // caller that can distinguish them: every other test here asserts a
        // *clean* shutdown, so a `waited` that reported success unconditionally
        // would pass all of them while hiding every transport error the binary
        // can hit.
        assert_eq!(
            waited(Ok::<(), std::io::Error>(())),
            ExitCode::SUCCESS,
            "a service that stopped cleanly exits 0"
        );
        assert_eq!(
            waited(Err::<(), &str>("transport closed unexpectedly")),
            ExitCode::FAILURE,
            "a serve error is exit 1, the documented transport-failure code"
        );
    }

    #[test]
    fn the_cli_revision_maps_to_the_served_one() {
        // The mapping is the flag's entire meaning; a wrong arm would serve
        // the other revision silently, and every wire test would still pass
        // because they all pass the flag they expect to be honoured.
        assert_eq!(
            ServedRevision::from(Revision::V20251125),
            ServedRevision::V2025_11_25
        );
        assert_eq!(
            ServedRevision::from(Revision::V20260728),
            ServedRevision::V2026_07_28
        );
        assert_eq!(
            ServedRevision::from(Revision::default()),
            ServedRevision::default(),
            "and the CLI default is the library default, not a second opinion"
        );
    }
}
