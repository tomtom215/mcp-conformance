// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The check that a client's JSON-RPC messages ride HTTP POSTs
//! (`TRAN-049` at `2025-11-25`, `TRAN-056` at `2026-07-28`).
//!
//! Both clauses say the same thing in the same words — *the client MUST use
//! HTTP POST to send JSON-RPC messages* — and both were excluded from judgment
//! for one reason: the recorded `http` event did not carry the verb. It does
//! now ([`EventBody::Http::method`]), so the obligation is judged rather than
//! documented as invisible.
//!
//! **What binds a message to a request.** A trace states one order, `seq`, and
//! the capture convention every recorder here follows is to write the request's
//! `http` event and then the message it carried. So the request a client
//! message rode is the nearest preceding client `http` event, and that is what
//! this check reads. It examines a message only where such an event exists and
//! records a method: a trace of bare messages with no HTTP framing (a
//! host-side capture, or stdio) carries no evidence about verbs, and reports
//! the clause *not observed* rather than passing it vacuously.
//!
//! **What stays excluded.** `TRAN-024`/`TRAN-055` add that each message must be
//! a *new* POST — one request per message. That is connection framing the trace
//! summarizes rather than reproduces, and no field added here recovers it, so
//! those two keep their exclusions with the framing as the stated reason.
//!
//! [`EventBody::Http::method`]: mcp_conformance_core::trace::EventBody

use mcp_conformance_core::trace::{Direction, EventBody, TransportKind};

use super::super::FindingSink;
use crate::context::TraceContext;

/// `TRAN-049` / `TRAN-056`: every client JSON-RPC message travels by POST.
pub(in crate::checks) fn client_messages_use_post(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let mut carrying: Option<(u64, &str)> = None;
    for event in context.events() {
        if event.transport != TransportKind::StreamableHttp
            || event.direction != Direction::ClientToServer
        {
            continue;
        }
        match &event.body {
            EventBody::Http { method, .. } => {
                carrying = method.as_deref().map(|method| (event.seq, method));
            }
            EventBody::Message { .. } => {
                let Some((request_seq, method)) = carrying else {
                    continue;
                };
                sink.examined();
                if method != "POST" {
                    sink.push(
                        Some(event.seq),
                        format!(
                            "client JSON-RPC message rode an HTTP {method} request (seq \
                             {request_seq}); messages must be sent by POST"
                        ),
                    );
                }
            }
            _ => {}
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

    const CHECK: &str = "transport.client-messages-use-post";

    fn outcome(lines: &[String]) -> (Vec<String>, u32) {
        let document = lines.join("\n");
        let events: Vec<TraceEvent> = parse_trace(&document, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        let outcome = checks::find(CHECK).unwrap().run(&context);
        (
            outcome
                .findings
                .into_iter()
                .map(|finding| finding.detail)
                .collect(),
            outcome.subjects,
        )
    }

    fn http(seq: u64, method: Option<&str>) -> String {
        let verb = method.map_or_else(String::new, |method| format!(r#""method":"{method}","#));
        format!(
            r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"http",{verb}"headers":{{}}}}"#
        )
    }

    fn message(seq: u64) -> String {
        format!(
            r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"method":"tools/list"}}}}"#
        )
    }

    #[test]
    fn a_message_carried_by_a_post_passes() {
        let (findings, subjects) = outcome(&[http(0, Some("POST")), message(1)]);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(subjects, 1);
    }

    #[test]
    fn a_message_carried_by_another_verb_is_a_violation() {
        let (findings, _) = outcome(&[http(0, Some("PUT")), message(1)]);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("PUT"), "{findings:?}");
        assert!(findings[0].contains("seq 0"), "{findings:?}");
    }

    #[test]
    fn a_teardown_delete_carries_no_message_and_is_not_judged() {
        // The DELETE follows a conforming POST exchange and carries nothing, so
        // nothing attributes it a message it never sent.
        let (findings, subjects) =
            outcome(&[http(0, Some("POST")), message(1), http(2, Some("DELETE"))]);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(subjects, 1, "only the POST's message was a subject");
    }

    #[test]
    fn a_stream_opening_get_is_not_credited_with_the_previous_posts_message() {
        // The GET resets what is carrying: a later message must not be blamed
        // on it, and it must not be excused by an earlier POST.
        let (findings, subjects) = outcome(&[
            http(0, Some("POST")),
            message(1),
            http(2, Some("GET")),
            message(3),
        ]);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("GET"), "{findings:?}");
        assert_eq!(subjects, 2);
    }

    #[test]
    fn messages_with_no_recorded_request_are_not_judged() {
        // A message-level capture (the host's own recorder, or stdio) carries
        // no verb to judge, so the clause reports not-observed.
        let (findings, subjects) = outcome(&[message(0), message(1)]);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(subjects, 0);

        // An HTTP event whose method the capture dropped is the same case.
        let (findings, subjects) = outcome(&[http(0, None), message(1)]);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(subjects, 0);
    }

    #[test]
    fn a_servers_reply_is_not_the_clients_message() {
        // The direction filter, pinned: a response arriving after the POST
        // that carried the request must not be counted as a second message
        // riding that POST. Without this the direction half of the scope guard
        // is unobserved, and a build that dropped it would report twice the
        // subjects it examined.
        let reply = r#"{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}}"#.to_owned();
        let (findings, subjects) = outcome(&[http(0, Some("POST")), message(1), reply]);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(subjects, 1, "only the client's message rode the POST");
    }

    #[test]
    fn stdio_messages_are_out_of_scope() {
        let stdio = [
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#.to_owned(),
        ];
        let (findings, subjects) = outcome(&stdio);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(subjects, 0);
    }
}
