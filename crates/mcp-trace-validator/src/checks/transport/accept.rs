// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The client `Accept`-header checks, one per request method.
//!
//! Streamable HTTP gives the client three request forms and binds a different
//! obligation to each:
//!
//! - **POST** carries messages, and its `Accept` MUST list *both*
//!   `application/json` and `text/event-stream` (`TRAN-025` at `2025-11-25`,
//!   `TRAN-057` at `2026-07-28`).
//! - **GET** opens a standalone stream, and its `Accept` MUST list
//!   `text/event-stream` (`TRAN-039`; `2026-07-28` removes the form).
//! - **DELETE** terminates a session and carries **no** `Accept` obligation at
//!   all — the clause that defines it (`basic/transports` §Session Management
//!   item 5) says only that the request names the session.
//!
//! Until 2026-08-20 one check enforced the *intersection* of the first two
//! obligations across the *union* of all three forms, because the recorded
//! event carried no method. That was wrong in both directions at once, and
//! both were reproduced against this repository's own reference host driving
//! its own reference server over HTTP:
//!
//! - The conforming session-teardown `DELETE` — `Accept: */*`, exactly what a
//!   client that owes nothing sends — was reported as failing `TRAN-025` *and*
//!   `TRAN-039`, two MUST-level failures against a client that had violated
//!   nothing.
//! - A client omitting `application/json` from a POST's `Accept`, which is the
//!   verbatim violation `TRAN-025` names, was reported `pass`, because the
//!   intersection only demanded the other media type.
//!
//! The method is now recorded ([`EventBody::Http::method`]) and each clause is
//! judged against exactly the requests it binds. An event whose method the
//! capture did not record is examined by neither: a recording that cannot tell
//! a POST from a DELETE can neither evidence a POST-only MUST nor convict on
//! one, so the clause reports *not observed* — the outcome this validator uses
//! everywhere else for a subject the trace does not carry.
//!
//! [`EventBody::Http::method`]: mcp_conformance_core::trace::EventBody

use std::collections::BTreeMap;

use mcp_conformance_core::trace::{Direction, EventBody};

use super::super::FindingSink;
use crate::context::TraceContext;

/// The client requests whose method the capture recorded, in trace order.
fn client_requests<'a>(
    context: &TraceContext<'a>,
) -> impl Iterator<Item = (u64, &'a str, &'a BTreeMap<String, String>)> {
    context
        .events()
        .iter()
        .filter(|event| event.direction == Direction::ClientToServer)
        .filter_map(|event| match &event.body {
            EventBody::Http {
                method: Some(method),
                headers,
                ..
            } => Some((event.seq, method.as_str(), headers)),
            _ => None,
        })
}

/// Whether an `Accept` field value offers `media`, case-insensitively.
///
/// Field values are matched by substring rather than parsed: the media types
/// at issue contain no character that could appear as a parameter value in a
/// conforming header, so a substring hit cannot be a false one, and `q`
/// parameters, ordering, and whitespace are all irrelevant to whether the type
/// was listed.
fn offers(accept: &str, media: &str) -> bool {
    accept.to_ascii_lowercase().contains(media)
}

/// `TRAN-025` (`2025-11-25`) / `TRAN-057` (`2026-07-28`): a client POST must
/// `Accept` both `application/json` and `text/event-stream`.
pub(in crate::checks) fn client_post_accept_header(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, method, headers) in client_requests(context) {
        if method != "POST" {
            continue;
        }
        sink.examined();
        let Some(accept) = headers.get("accept") else {
            sink.push(
                Some(seq),
                "client HTTP POST has no Accept header; it must list both \
                 application/json and text/event-stream"
                    .to_owned(),
            );
            continue;
        };
        let missing: Vec<&str> = ["application/json", "text/event-stream"]
            .into_iter()
            .filter(|media| !offers(accept, media))
            .collect();
        if !missing.is_empty() {
            sink.push(
                Some(seq),
                format!(
                    "client POST Accept header {accept:?} does not list {}; \
                     a POST must offer both application/json and text/event-stream",
                    missing.join(" or ")
                ),
            );
        }
    }
}

/// `TRAN-039`: a client GET opening a standalone stream must `Accept`
/// `text/event-stream`.
pub(in crate::checks) fn client_get_accept_header(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, method, headers) in client_requests(context) {
        if method != "GET" {
            continue;
        }
        sink.examined();
        match headers.get("accept") {
            None => sink.push(
                Some(seq),
                "client HTTP GET has no Accept header; a GET to the MCP endpoint \
                 must list text/event-stream"
                    .to_owned(),
            ),
            Some(accept) if !offers(accept, "text/event-stream") => sink.push(
                Some(seq),
                format!("client GET Accept header {accept:?} does not list text/event-stream"),
            ),
            Some(_) => {}
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use mcp_conformance_core::trace::TraceEvent;

    use crate::checks;
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};

    /// One client HTTP request event, with whatever method and headers.
    fn request(seq: u64, method: &str, headers: &str) -> String {
        format!(
            r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"{method}","headers":{headers}}}"#
        )
    }

    fn run(check: &str, lines: &[String]) -> Vec<String> {
        let document = lines.join("\n");
        let events: Vec<TraceEvent> = parse_trace(&document, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check)
            .unwrap()
            .run(&context)
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    /// Whether the check examined any subject at all, which is what separates
    /// `not observed` from `pass` in the report.
    fn examined(check: &str, lines: &[String]) -> u32 {
        let document = lines.join("\n");
        let events: Vec<TraceEvent> = parse_trace(&document, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check).unwrap().run(&context).subjects
    }

    const BOTH: &str = r#"{"accept":"application/json, text/event-stream"}"#;

    #[test]
    fn a_post_must_offer_both_media_types() {
        let full = [request(0, "POST", BOTH)];
        assert!(run("transport.client-post-accept-header", &full).is_empty());

        // The half the old single check could not see: `text/event-stream`
        // alone satisfied it, though TRAN-025 names both.
        let stream_only = [request(0, "POST", r#"{"accept":"text/event-stream"}"#)];
        let findings = run("transport.client-post-accept-header", &stream_only);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("application/json"), "{findings:?}");

        let json_only = [request(0, "POST", r#"{"accept":"application/json"}"#)];
        let findings = run("transport.client-post-accept-header", &json_only);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("text/event-stream"), "{findings:?}");

        let none = [request(0, "POST", "{}")];
        let findings = run("transport.client-post-accept-header", &none);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("no Accept header"), "{findings:?}");
    }

    #[test]
    fn a_get_must_offer_the_event_stream_only() {
        let stream_only = [request(0, "GET", r#"{"accept":"text/event-stream"}"#)];
        assert!(run("transport.client-get-accept-header", &stream_only).is_empty());

        let json_only = [request(0, "GET", r#"{"accept":"application/json"}"#)];
        let findings = run("transport.client-get-accept-header", &json_only);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("text/event-stream"), "{findings:?}");

        let none = [request(0, "GET", "{}")];
        let findings = run("transport.client-get-accept-header", &none);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("no Accept header"), "{findings:?}");
    }

    #[test]
    fn a_session_teardown_delete_owes_no_accept_header() {
        // The exact exchange the reference host sends at teardown, which both
        // clauses used to fail: reqwest's default `Accept: */*` on a DELETE.
        let teardown = [
            request(0, "POST", BOTH),
            request(1, "DELETE", r#"{"accept":"*/*","mcp-session-id":"abc123"}"#),
        ];
        assert!(
            run("transport.client-post-accept-header", &teardown).is_empty(),
            "a DELETE is not a POST"
        );
        assert!(
            run("transport.client-get-accept-header", &teardown).is_empty(),
            "a DELETE is not a GET"
        );

        // A DELETE with no Accept header at all is equally conforming.
        let bare = [request(0, "DELETE", r#"{"mcp-session-id":"abc123"}"#)];
        assert!(run("transport.client-post-accept-header", &bare).is_empty());
        assert!(run("transport.client-get-accept-header", &bare).is_empty());
    }

    #[test]
    fn a_request_whose_method_was_not_recorded_is_not_judged() {
        // No method: neither check may convict, and neither may claim to have
        // examined anything — the clause reports `not observed`, not `pass`.
        let methodless = [
            r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","headers":{"accept":"*/*"}}"#
                .to_owned(),
        ];
        for check in [
            "transport.client-post-accept-header",
            "transport.client-get-accept-header",
        ] {
            assert!(run(check, &methodless).is_empty(), "{check}");
            assert_eq!(examined(check, &methodless), 0, "{check}");
        }
    }

    #[test]
    fn method_matching_survives_a_lowercasing_capturer() {
        let lowercased = [request(0, "post", r#"{"accept":"application/json"}"#)];
        let findings = run("transport.client-post-accept-header", &lowercased);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn accept_matching_ignores_order_case_and_parameters() {
        let fussy = [request(
            0,
            "POST",
            r#"{"accept":"TEXT/EVENT-STREAM;q=0.9, Application/JSON;q=1.0"}"#,
        )];
        assert!(run("transport.client-post-accept-header", &fussy).is_empty());
    }
}
