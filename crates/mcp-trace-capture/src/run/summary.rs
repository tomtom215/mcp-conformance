// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a run says on stderr when it ends: what went unrecorded, and where to
//! point the client.

use std::net::SocketAddr;

use super::{EXIT_USAGE, HostHeader};
use crate::output::EXIT_INCOMPLETE;
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
    fail_on_unrecorded: bool,
) -> i32 {
    match served {
        Ok(unrecorded) => {
            let lines = http_text_lines(&unrecorded)
                .into_iter()
                .chain(unrecorded_lines("session", unrecorded.oversized))
                .chain(http_gaps(&unrecorded));
            for line in lines {
                eprintln!("mcp-trace-capture: {line}");
            }
            report_statuses(unrecorded, bound, upstream);
            refused_requests_code(&unrecorded, fail_on_unrecorded)
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            EXIT_USAGE
        }
    }
}

/// What the proxy recorded as the text it was, one line per kind that
/// occurred: a body that is not JSON, and JSON the trace cannot hold as sent.
fn http_text_lines(unrecorded: &http::Unrecorded) -> Vec<String> {
    text_lines(
        "session message(s)",
        unrecorded.not_json,
        unrecorded.altered,
    )
}

/// What the proxy forwarded but did not record, or answered itself, one line
/// per kind that occurred: the messages past the buffer budget, SSE events of
/// another type, and the requests the upstream never answered in full or never
/// saw.
fn http_gaps(unrecorded: &http::Unrecorded) -> Vec<String> {
    let mut lines = Vec::new();
    if unrecorded.over_budget > 0 {
        lines.push(format!(
            "{} session message(s) arrived while the proxy already held its buffer budget \
             (4 x --max-message-bytes) of other messages and are not in the trace (forwarded \
             unchanged)",
            unrecorded.over_budget
        ));
    }
    if unrecorded.other_events > 0 {
        lines.push(format!(
            "{} SSE event(s) had a type other than `message`, which MCP clients ignore, and \
             are not in the trace (forwarded unchanged)",
            unrecorded.other_events
        ));
    }
    if unrecorded.upstream_failures > 0 {
        lines.push(format!(
            "{} request(s) could not reach the upstream and were answered 502 by the proxy \
             (recorded as transport-abort)",
            unrecorded.upstream_failures
        ));
    }
    if unrecorded.upstream_cut > 0 {
        lines.push(format!(
            "{} response(s) were cut off by the upstream mid-body; the truncation was relayed \
             (recorded as transport-abort)",
            unrecorded.upstream_cut
        ));
    }
    if unrecorded.refused_paths > 0 {
        lines.push(format!(
            "{} request(s) had a `.` or `..` path segment, or a path not UTF-8 once decoded, \
             and were answered 400 by the proxy, not forwarded (not in the trace)",
            unrecorded.refused_paths
        ));
    }
    if unrecorded.refused_connects > 0 {
        lines.push(format!(
            "{} CONNECT request(s) asked for a tunnel and were answered 405 by the proxy, not \
             forwarded (not in the trace)",
            unrecorded.refused_connects
        ));
    }
    if unrecorded.stalled_bodies > 0 {
        lines.push(format!(
            "{} request(s) sent no body byte within the body read timeout and were answered \
             408 by the proxy, their connection closed (recorded as transport-abort)",
            unrecorded.stalled_bodies
        ));
    }
    lines
}

/// The exit code for what the proxy refused on its own: [`EXIT_INCOMPLETE`] with
/// `--fail-on-unrecorded`, since those requests are in no session's trace (no
/// server ever saw them), else 0.
fn refused_requests_code(unrecorded: &http::Unrecorded, fail_on_unrecorded: bool) -> i32 {
    let refused = unrecorded.refused_paths + unrecorded.refused_connects;
    if fail_on_unrecorded && refused > 0 {
        eprintln!(
            "mcp-trace-capture: --fail-on-unrecorded: {refused} request(s) were refused by the \
             proxy and are in no trace"
        );
        return EXIT_INCOMPLETE;
    }
    0
}

/// Says how many responses were not 2xx, asks about the path when the server
/// answered 404 (a client pointed at the wrong path gets 404s, recorded
/// faithfully and otherwise unremarked — but not one the MCP server itself
/// sent), and names the flag that stops forwarding a foreign `Host` when the
/// server refused one with 403.
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
    if unrecorded.forbidden_foreign_host > 0 {
        eprintln!(
            "mcp-trace-capture: the server answered 403 to {} request(s) whose Host header \
             (a name other than the server's own) was passed through to it; if that is its \
             Host/Origin check, --no-preserve-host sends the server's own name instead",
            unrecorded.forbidden_foreign_host
        );
    }
}

/// The stdio summary: non-JSON lines are in the trace (as string payloads), so
/// they are reported as findings to look for rather than as gaps in it. One
/// line per kind that occurred.
pub(super) fn stdio_lines(side: &str, unrecorded: stdio::Unrecorded) -> Vec<String> {
    let mut lines = text_lines(
        &format!("{side} line(s)"),
        unrecorded.not_json,
        unrecorded.altered,
    );
    if unrecorded.unterminated > 0 {
        lines.push(format!(
            "the {side}'s output ended mid-line, without the newline that frames a stdio \
             message; the remainder is recorded as a string payload, which the validator \
             reports as not a valid MCP message"
        ));
    }
    lines.extend(unrecorded_lines(side, unrecorded.oversized));
    lines
}

/// What was recorded as the text it was, which the validator then judges as
/// what it is: one line per kind that occurred. `subject` names what the counts
/// are of (`client line(s)`, `session message(s)`).
fn text_lines(subject: &str, not_json: u64, altered: u64) -> Vec<String> {
    let mut lines = Vec::new();
    if not_json > 0 {
        lines.push(format!(
            "{not_json} {subject} were not JSON; each is recorded as a string payload, which \
             the validator reports as not a valid MCP message"
        ));
    }
    if altered > 0 {
        lines.push(format!(
            "{altered} {subject} were JSON the trace cannot hold as sent ({ALTERED}); each is \
             recorded as a string payload, which the validator reports as not a valid MCP message"
        ));
    }
    lines
}

/// Why a JSON message cannot be recorded as parsed JSON.
const ALTERED: &str = "a number a double cannot hold exactly, such as an integer past 64 bits, or a repeated \
     member name";

/// Messages forwarded but left out of the trace for their size.
fn unrecorded_lines(side: &str, oversized: u64) -> Vec<String> {
    let mut lines = Vec::new();
    if oversized > 0 {
        lines.push(format!(
            "{oversized} {side} message(s) were over the size limit (--max-message-bytes) and \
             are not in the trace (forwarded unchanged)"
        ));
    }
    lines
}

/// Where the proxy listens, what to point the client at, and what the server
/// sees as `Host` — warning first when the address is not loopback.
pub(super) fn announcement(
    bound: SocketAddr,
    upstream: &axum::http::Uri,
    host: HostHeader,
) -> Vec<String> {
    let mut lines = Vec::new();
    if !bound.ip().is_loopback() {
        lines.push(format!(
            "warning: {bound} is not a loopback address; anyone who can reach it can send \
             requests through this proxy, credentials and all, and have them recorded"
        ));
    }
    lines.push(format!(
        "listening on http://{bound}, forwarding to {} (Ctrl-C to stop)",
        shown(upstream)
    ));
    lines.push(client_url(bound, upstream));
    lines.push(host_line(host, http::is_loopback(upstream)).to_owned());
    lines
}

/// What the server sees as `Host`, and why: a server that checks `Host`
/// against DNS rebinding judges whatever the proxy sends it.
const fn host_line(host: HostHeader, loopback: bool) -> &'static str {
    match host {
        HostHeader::Client => {
            "sending the client's Host header through to the server (--preserve-host), so its \
             DNS-rebinding check judges what the client sent"
        }
        HostHeader::Upstream => {
            "sending the server's own name as Host (--no-preserve-host); its DNS-rebinding \
             check sees that, not what the client sent"
        }
        HostHeader::Auto if loopback => {
            "sending the client's Host header through to the server, which is on this \
             machine, so its DNS-rebinding check judges what the client sent \
             (--no-preserve-host sends the server's own name instead)"
        }
        HostHeader::Auto => {
            "sending the server's own name as Host, since it is not on this machine and may \
             route on it; its DNS-rebinding check sees that, not what the client sent \
             (--preserve-host passes the client's through)"
        }
    }
}

#[cfg(test)]
mod tests;
