// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

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
        altered: 1,
        oversized: 1,
        not_2xx: 1,
        not_found: 1,
        forbidden_foreign_host: 1,
        ..http::Unrecorded::default()
    };
    assert_eq!(http_gaps(&reported_elsewhere), Vec::<String>::new());
}

/// What the proxy refused on its own is in no session's trace, so only the
/// flag turns it into an exit code, and either kind of refusal does.
#[test]
fn a_refused_request_fails_the_run_only_with_the_flag() {
    let none = http::Unrecorded::default();
    for refused in [
        http::Unrecorded {
            refused_paths: 1,
            ..none
        },
        http::Unrecorded {
            refused_connects: 1,
            ..none
        },
        http::Unrecorded {
            refused_paths: 2,
            refused_connects: 3,
            ..none
        },
    ] {
        assert_eq!(refused_requests_code(&refused, true), EXIT_INCOMPLETE);
        assert_eq!(refused_requests_code(&refused, false), 0);
    }
    assert_eq!(refused_requests_code(&none, true), 0);
}

#[test]
fn messages_left_out_are_counted() {
    assert_eq!(
        unrecorded_lines("session", 4),
        [
            "4 session message(s) were over the size limit (--max-message-bytes) and are \
             not in the trace (forwarded unchanged)",
        ]
    );
    assert_eq!(unrecorded_lines("session", 0), Vec::<String>::new());
}

/// What the proxy records as the text it was — a body that is not JSON, or
/// JSON the trace cannot hold as sent — is said to be in the trace, as
/// stdio says it of its lines: a finding to look for, not a gap.
#[test]
fn what_the_proxy_records_as_text_is_said_to_be_in_the_trace() {
    let none = http::Unrecorded::default();
    assert_eq!(http_text_lines(&none), Vec::<String>::new());
    let lines = http_text_lines(&http::Unrecorded {
        not_json: 3,
        altered: 2,
        ..none
    });
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("3 session message(s) were not JSON; each is recorded as a string"),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("2 session message(s) were JSON the trace cannot hold as sent")
            && lines[1].contains("each is recorded as a string payload"),
        "{lines:?}"
    );
    for line in &lines {
        assert!(!line.contains("not in the trace"), "{line}");
    }
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
