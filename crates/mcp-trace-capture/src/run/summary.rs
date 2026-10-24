// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a run says on stderr when it ends: what went unrecorded, and where to
//! point the client.

use std::net::SocketAddr;

use super::{EXIT_USAGE, HostHeader};
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
            let unrecorded_lines =
                unrecorded_lines("session", unrecorded.not_json, unrecorded.oversized);
            for line in unrecorded_lines.into_iter().chain(http_gaps(&unrecorded)) {
                eprintln!("mcp-trace-capture: {line}");
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

/// What the proxy forwarded but did not record, or answered itself, one line
/// per kind that occurred: the messages the trace cannot hold, those past the
/// buffer budget, SSE events of another type, and the requests the upstream
/// never answered in full or never saw.
fn http_gaps(unrecorded: &http::Unrecorded) -> Vec<String> {
    let mut lines = Vec::new();
    if unrecorded.altered > 0 {
        lines.push(format!(
            "{} session message(s) were JSON the trace cannot hold as sent ({ALTERED}) and are \
             not in the trace (forwarded unchanged)",
            unrecorded.altered
        ));
    }
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
/// they are reported as findings to look for rather than as gaps in it. One
/// line per kind that occurred.
pub(super) fn stdio_lines(side: &str, unrecorded: stdio::Unrecorded) -> Vec<String> {
    let mut lines = Vec::new();
    if unrecorded.not_json > 0 {
        lines.push(format!(
            "{} {side} line(s) were not JSON; each is recorded as a string payload, which the \
             validator reports as not a valid MCP message",
            unrecorded.not_json
        ));
    }
    if unrecorded.altered > 0 {
        lines.push(format!(
            "{} {side} line(s) were JSON the trace cannot hold as sent ({ALTERED}); each is \
             recorded as a string payload, which the validator reports as not a valid MCP message",
            unrecorded.altered
        ));
    }
    if unrecorded.unterminated > 0 {
        lines.push(format!(
            "the {side}'s output ended mid-line, without the newline that frames a stdio \
             message; the remainder is recorded as a string payload, which the validator \
             reports as not a valid MCP message"
        ));
    }
    lines.extend(unrecorded_lines(side, 0, unrecorded.oversized));
    lines
}

/// Why a JSON message cannot be recorded as parsed JSON.
const ALTERED: &str = "a number a double cannot hold exactly, such as an integer past 64 bits, or a repeated \
     member name";

/// Messages forwarded but left out of the trace: not JSON, or over the size
/// limit; one line per kind that occurred.
fn unrecorded_lines(side: &str, not_json: u64, oversized: u64) -> Vec<String> {
    let mut lines = Vec::new();
    if not_json > 0 {
        lines.push(format!(
            "{not_json} {side} message(s) were not JSON and are not in the trace (forwarded \
             unchanged)"
        ));
    }
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // One table, with a row per gap the summary can name; splitting it would
    // hide that every row is checked the same way.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn each_gap_that_occurred_is_named_once_and_no_other() {
        assert_eq!(
            http_gaps(&http::Unrecorded::default()),
            Vec::<String>::new()
        );
        let none = http::Unrecorded::default();
        for (unrecorded, start) in [
            (
                http::Unrecorded { altered: 2, ..none },
                "2 session message(s) were JSON the trace cannot hold",
            ),
            (
                http::Unrecorded {
                    over_budget: 2,
                    ..none
                },
                "2 session message(s) arrived while the proxy already held its buffer budget",
            ),
            (
                http::Unrecorded {
                    other_events: 2,
                    ..none
                },
                "2 SSE event(s) had a type other than `message`",
            ),
            (
                http::Unrecorded {
                    upstream_failures: 2,
                    ..none
                },
                "2 request(s) could not reach the upstream",
            ),
            (
                http::Unrecorded {
                    upstream_cut: 2,
                    ..none
                },
                "2 response(s) were cut off by the upstream mid-body",
            ),
            (
                http::Unrecorded {
                    refused_paths: 2,
                    ..none
                },
                "2 request(s) had a `.` or `..` path segment, or a path not UTF-8",
            ),
            (
                http::Unrecorded {
                    refused_connects: 2,
                    ..none
                },
                "2 CONNECT request(s) asked for a tunnel and were answered 405",
            ),
            (
                http::Unrecorded {
                    stalled_bodies: 2,
                    ..none
                },
                "2 request(s) sent no body byte within the body read timeout",
            ),
        ] {
            let lines = http_gaps(&unrecorded);
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert!(lines[0].starts_with(start), "{lines:?}");
        }
        // Counts that are recorded elsewhere name no gap here.
        let reported_elsewhere = http::Unrecorded {
            not_json: 1,
            oversized: 1,
            not_2xx: 1,
            not_found: 1,
            ..http::Unrecorded::default()
        };
        assert_eq!(http_gaps(&reported_elsewhere), Vec::<String>::new());
    }

    #[test]
    fn messages_left_out_are_counted_by_kind() {
        assert_eq!(
            unrecorded_lines("session", 3, 4),
            [
                "3 session message(s) were not JSON and are not in the trace (forwarded \
                 unchanged)",
                "4 session message(s) were over the size limit (--max-message-bytes) and are \
                 not in the trace (forwarded unchanged)",
            ]
        );
        assert_eq!(unrecorded_lines("session", 0, 0), Vec::<String>::new());
    }

    #[test]
    fn each_stdio_finding_that_occurred_is_named_once_and_no_other() {
        let none = stdio::Unrecorded::default();
        assert_eq!(stdio_lines("client", none), Vec::<String>::new());
        for (unrecorded, start) in [
            (
                stdio::Unrecorded {
                    not_json: 2,
                    ..none
                },
                "2 client line(s) were not JSON",
            ),
            (
                stdio::Unrecorded { altered: 2, ..none },
                "2 client line(s) were JSON the trace cannot hold",
            ),
            (
                stdio::Unrecorded {
                    unterminated: 1,
                    ..none
                },
                "the client's output ended mid-line",
            ),
            (
                stdio::Unrecorded {
                    oversized: 2,
                    ..none
                },
                "2 client message(s) were over the size limit",
            ),
        ] {
            let lines = stdio_lines("client", unrecorded);
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert!(lines[0].starts_with(start), "{lines:?}");
        }
    }

    #[test]
    fn the_announcement_names_the_address_and_warns_off_loopback_only() {
        let upstream: axum::http::Uri = "http://upstream.test/mcp?key=secret".parse().unwrap();
        let local: SocketAddr = "127.0.0.1:4100".parse().unwrap();
        assert_eq!(
            announcement(local, &upstream, HostHeader::Auto),
            [
                "listening on http://127.0.0.1:4100, forwarding to http://upstream.test/mcp?… \
                 (Ctrl-C to stop)"
                    .to_owned(),
                client_url(local, &upstream),
                host_line(HostHeader::Auto, false).to_owned(),
            ]
        );
        let open: SocketAddr = "0.0.0.0:4100".parse().unwrap();
        let lines = announcement(open, &upstream, HostHeader::Auto);
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(
            lines[0].starts_with("warning: 0.0.0.0:4100 is not a loopback address"),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("listening on http://0.0.0.0:4100,"));
    }

    /// The `Host` line says what the server sees, by the flag given or else by
    /// where the upstream is, and names the flag that would change it.
    #[test]
    fn the_host_line_says_what_the_server_sees_and_why() {
        let loopback: axum::http::Uri = "http://127.0.0.1:3000/mcp".parse().unwrap();
        let local: SocketAddr = "127.0.0.1:4100".parse().unwrap();
        let last = |host| announcement(local, &loopback, host).pop().unwrap();
        assert!(last(HostHeader::Auto).starts_with("sending the client's Host header through"));
        assert!(last(HostHeader::Auto).contains("--no-preserve-host"));
        assert!(last(HostHeader::Client).starts_with("sending the client's Host header through"));
        assert!(last(HostHeader::Client).contains("(--preserve-host)"));
        assert!(last(HostHeader::Upstream).starts_with("sending the server's own name as Host"));
        assert!(last(HostHeader::Upstream).contains("(--no-preserve-host)"));
        let remote = host_line(HostHeader::Auto, false);
        assert!(
            remote.starts_with("sending the server's own name as Host"),
            "{remote}"
        );
        assert!(remote.contains("--preserve-host passes"), "{remote}");
        assert_ne!(host_line(HostHeader::Auto, true), remote);
    }
}
