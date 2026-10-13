// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a run says on stderr when it ends: what went unrecorded, and where to
//! point the client.

use std::net::SocketAddr;

use super::EXIT_USAGE;
use crate::{http, stdio};

/// The upstream as announced: without its query, which can carry credentials.
pub(super) fn shown(upstream: &axum::http::Uri) -> String {
    let path = upstream.path();
    let authority = upstream
        .authority()
        .map_or("", |authority| authority.as_str());
    let scheme = upstream.scheme_str().unwrap_or("http");
    let query = if upstream.query().is_some() {
        "?…"
    } else {
        ""
    };
    format!("{scheme}://{authority}{path}{query}")
}

/// Where to point the client: the proxy's root is the upstream URL itself.
pub(super) fn client_url(bound: SocketAddr, upstream: &axum::http::Uri) -> String {
    if upstream.path() == "/" {
        format!(
            "client URL: http://{bound}/ plus the server's endpoint path, e.g. \
             http://{bound}/mcp (each request's path is appended to the upstream's)"
        )
    } else {
        format!(
            "client URL: http://{bound}/ (it reaches {}; http://{bound}/<path> reaches \
             <upstream>/<path>)",
            shown(upstream)
        )
    }
}

pub(super) fn http_summary(
    served: std::io::Result<http::Unrecorded>,
    bound: SocketAddr,
    upstream: &axum::http::Uri,
) -> i32 {
    match served {
        Ok(unrecorded) => {
            report_unrecorded("session", unrecorded.not_json, unrecorded.oversized);
            if unrecorded.altered > 0 {
                eprintln!(
                    "mcp-trace-capture: {} session message(s) were JSON the trace cannot hold \
                     as sent ({ALTERED}) and are not in the trace (forwarded unchanged)",
                    unrecorded.altered
                );
            }
            if unrecorded.over_budget > 0 {
                eprintln!(
                    "mcp-trace-capture: {} session message(s) arrived while the proxy already \
                     held its buffer budget (4 x --max-message-bytes) of other messages and are \
                     not in the trace (forwarded unchanged)",
                    unrecorded.over_budget
                );
            }
            if unrecorded.other_events > 0 {
                eprintln!(
                    "mcp-trace-capture: {} SSE event(s) had a type other than `message`, which \
                     MCP clients ignore, and are not in the trace (forwarded unchanged)",
                    unrecorded.other_events
                );
            }
            if unrecorded.upstream_failures > 0 {
                eprintln!(
                    "mcp-trace-capture: {} request(s) could not reach the upstream and were \
                     answered 502 by the proxy (recorded as transport-abort)",
                    unrecorded.upstream_failures
                );
            }
            if unrecorded.upstream_cut > 0 {
                eprintln!(
                    "mcp-trace-capture: {} response(s) were cut off by the upstream mid-body; \
                     the truncation was relayed (recorded as transport-abort)",
                    unrecorded.upstream_cut
                );
            }
            if unrecorded.refused_paths > 0 {
                eprintln!(
                    "mcp-trace-capture: {} request(s) had a `.` or `..` path segment and were \
                     answered 400 by the proxy, not forwarded (not in the trace)",
                    unrecorded.refused_paths
                );
            }
            report_statuses(unrecorded, bound, upstream);
            0
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            EXIT_USAGE
        }
    }
}

/// Says how many responses were not 2xx, and asks about the path when the
/// server answered 404: a client pointed at the wrong path gets 404s, recorded
/// faithfully and otherwise unremarked.
fn report_statuses(unrecorded: http::Unrecorded, bound: SocketAddr, upstream: &axum::http::Uri) {
    if unrecorded.not_2xx > 0 {
        eprintln!(
            "mcp-trace-capture: {} response(s) had a status outside 2xx (recorded as sent)",
            unrecorded.not_2xx
        );
    }
    if unrecorded.not_found > 0 {
        eprintln!(
            "mcp-trace-capture: the server answered 404 to {} request(s) — is the endpoint \
             path right? The proxy maps http://{bound}/ to {} and http://{bound}/<path> to \
             <upstream>/<path>",
            unrecorded.not_found,
            shown(upstream)
        );
    }
}

/// The stdio summary: non-JSON lines are in the trace (as string payloads), so
/// they are reported as findings to look for rather than as gaps in it.
pub(super) fn report_stdio(side: &str, unrecorded: stdio::Unrecorded) {
    if unrecorded.not_json > 0 {
        eprintln!(
            "mcp-trace-capture: {} {side} line(s) were not JSON; each is recorded as a \
             string payload, which the validator reports as not a valid MCP message",
            unrecorded.not_json
        );
    }
    if unrecorded.altered > 0 {
        eprintln!(
            "mcp-trace-capture: {} {side} line(s) were JSON the trace cannot hold as sent \
             ({ALTERED}); each is recorded as a string payload, which the validator reports as \
             not a valid MCP message",
            unrecorded.altered
        );
    }
    if unrecorded.unterminated > 0 {
        eprintln!(
            "mcp-trace-capture: the {side}'s output ended mid-line, without the newline that \
             frames a stdio message; the remainder is recorded as a string payload, which the \
             validator reports as not a valid MCP message"
        );
    }
    report_unrecorded(side, 0, unrecorded.oversized);
}

/// Why a JSON message cannot be recorded as parsed JSON.
const ALTERED: &str = "a number a double cannot hold exactly, such as an integer past 64 bits, or a repeated \
     member name";

fn report_unrecorded(side: &str, not_json: u64, oversized: u64) {
    if not_json > 0 {
        eprintln!(
            "mcp-trace-capture: {not_json} {side} message(s) were not JSON and are not in \
             the trace (forwarded unchanged)"
        );
    }
    if oversized > 0 {
        eprintln!(
            "mcp-trace-capture: {oversized} {side} message(s) were over the size limit \
             (--max-message-bytes) and are not in the trace (forwarded unchanged)"
        );
    }
}

/// Where the proxy listens and what to point the client at, warning first
/// when that is not loopback.
pub(super) fn announce(bound: SocketAddr, upstream: &axum::http::Uri) {
    if !bound.ip().is_loopback() {
        eprintln!(
            "mcp-trace-capture: warning: {bound} is not a loopback address; anyone who can \
             reach it can send requests through this proxy, credentials and all, and have \
             them recorded"
        );
    }
    eprintln!(
        "mcp-trace-capture: listening on http://{bound}, forwarding to {} (Ctrl-C to stop)",
        shown(upstream)
    );
    eprintln!("mcp-trace-capture: {}", client_url(bound, upstream));
}
