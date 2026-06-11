// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Request/response pairing: which request each response answers.
//!
//! Feature-area checks judge *exchanges* — "the result of a `tools/list` request MUST
//! …" — so the context precomputes, in one deterministic pass, the request event each
//! result or error response corresponds to. Pairing is deliberately lenient about
//! everything other checks already police: a duplicate request ID still pairs (with the
//! earliest unanswered request, the only reading that lets later checks point at both
//! events), and a response nobody asked for simply pairs with nothing.

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::TraceEvent;
use serde_json::Value;

use super::TraceContext;

/// One completed request/response exchange, in response order.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Exchange<'a> {
    /// The request event.
    pub request: &'a TraceEvent,
    /// The request's `method`.
    pub method: &'a str,
    /// The request's `params`, when present.
    pub params: Option<&'a Value>,
    /// The response event (result or error).
    pub response: &'a TraceEvent,
    /// The response's `result` value; `None` when the response is an error.
    pub result: Option<&'a Value>,
}

/// For each event, the index of the request it answers — `Some` only for result/error
/// responses whose ID matches an earlier unanswered request from the opposite
/// direction.
pub(super) fn pair_responses(
    events: &[TraceEvent],
    kinds: &[Option<MessageKind<'_>>],
) -> Vec<Option<usize>> {
    let mut open: Vec<usize> = Vec::new();
    let mut pairs = vec![None; events.len()];
    for (index, (event, kind)) in events.iter().zip(kinds).enumerate() {
        match kind {
            Some(MessageKind::Request { .. }) => open.push(index),
            Some(
                MessageKind::Result { id: Some(id) } | MessageKind::Error { id: Some(id), .. },
            ) => {
                let answered = open.iter().position(|&request_index| {
                    let request = &events[request_index];
                    request.direction != event.direction
                        && matches!(
                            &kinds[request_index],
                            Some(MessageKind::Request { id: request_id, .. })
                                if *request_id == *id
                        )
                });
                if let Some(position) = answered {
                    pairs[index] = Some(open.remove(position));
                }
            }
            _ => {}
        }
    }
    pairs
}

impl<'a> TraceContext<'a> {
    /// Iterates completed request/response exchanges, in response order.
    pub fn exchanges(&self) -> impl Iterator<Item = Exchange<'a>> + '_ {
        self.pairs
            .iter()
            .enumerate()
            .filter_map(move |(index, request_index)| {
                let request_index = (*request_index)?;
                let request = &self.events[request_index];
                let Some(MessageKind::Request { method, .. }) = &self.kinds[request_index] else {
                    return None;
                };
                let response = &self.events[index];
                Some(Exchange {
                    request,
                    method,
                    params: request
                        .message_payload()
                        .and_then(|payload| payload.get("params")),
                    response,
                    result: response
                        .message_payload()
                        .and_then(|payload| payload.get("result")),
                })
            })
    }

    /// Completed exchanges for one request method — the shape feature-area checks
    /// want: "every `tools/list` result MUST …".
    pub fn exchanges_for(&self, method: &'a str) -> impl Iterator<Item = Exchange<'a>> + '_ {
        self.exchanges()
            .filter(move |exchange| exchange.method == method)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};
    use mcp_conformance_core::trace::TraceEvent;

    fn line(seq: u64, direction: &str, payload: &str) -> String {
        format!(
            r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
        )
    }

    fn events_of(lines: &[String]) -> Vec<TraceEvent> {
        parse_trace(&lines.join("\n"), &Limits::default()).unwrap()
    }

    #[test]
    fn pairs_results_and_errors_with_their_requests() {
        let events = events_of(&[
            line(
                0,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ),
            line(
                1,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo"}}"#,
            ),
            line(
                2,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"x"}}"#,
            ),
            line(
                3,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        let exchanges: Vec<_> = context.exchanges().collect();
        assert_eq!(exchanges.len(), 2);
        // Response order: the error to id 2 comes first.
        assert_eq!(exchanges[0].method, "tools/call");
        assert_eq!(exchanges[0].request.seq, 1);
        assert_eq!(exchanges[0].response.seq, 2);
        assert!(
            exchanges[0].result.is_none(),
            "error responses have no result"
        );
        assert!(exchanges[0].params.is_some());
        assert_eq!(exchanges[1].method, "tools/list");
        assert_eq!(exchanges[1].response.seq, 3);
        assert_eq!(
            exchanges[1].result.unwrap(),
            &serde_json::json!({"tools": []})
        );
        assert!(exchanges[1].params.is_none());
    }

    #[test]
    fn responses_pair_only_against_the_opposite_direction() {
        // A "response" travelling the same way as the request answers nothing.
        let events = events_of(&[
            line(
                0,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ),
            line(
                1,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        assert_eq!(context.exchanges().count(), 0);
    }

    #[test]
    fn duplicate_request_ids_pair_earliest_first() {
        let events = events_of(&[
            line(
                0,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
            ),
            line(
                1,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#,
            ),
            line(
                2,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":7,"result":{}}"#,
            ),
            line(
                3,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":7,"result":{"tools":[]}}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        let methods: Vec<&str> = context
            .exchanges()
            .map(|exchange| exchange.method)
            .collect();
        assert_eq!(methods, ["ping", "tools/list"]);
    }

    #[test]
    fn unanswered_and_unsolicited_messages_pair_with_nothing() {
        let events = events_of(&[
            line(
                0,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ),
            line(
                1,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":99,"result":{}}"#,
            ),
            line(2, "server-to-client", r#"{"jsonrpc":"2.0","result":{}}"#),
            line(
                3,
                "client-to-server",
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        assert_eq!(context.exchanges().count(), 0);
    }

    #[test]
    fn an_id_is_answered_once_then_reopens_for_nothing() {
        // After a request is answered, a second response with the same id pairs with
        // nothing (the request is no longer open).
        let events = events_of(&[
            line(
                0,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            ),
            line(
                1,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            ),
            line(
                2,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        assert_eq!(context.exchanges().count(), 1);
    }

    #[test]
    fn exchanges_for_filters_by_method() {
        let events = events_of(&[
            line(
                0,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            ),
            line(
                1,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            ),
            line(
                2,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            ),
            line(
                3,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        assert_eq!(context.exchanges_for("tools/list").count(), 1);
        assert_eq!(context.exchanges_for("ping").count(), 1);
        assert_eq!(context.exchanges_for("prompts/list").count(), 0);
    }

    #[test]
    fn a_response_preceding_its_request_pairs_with_nothing() {
        // Capture order is the only authority (seq is strictly increasing). A
        // result at seq 0 cannot answer a request that first appears at seq 1
        // — you cannot answer before you ask — so it pairs with nothing, and
        // the exchange-based content checks abstain on it. This is the
        // deliberate lenient-pairing contract: the orphan response is BASE-004's
        // to flag, not a content check's; pinned here so it cannot drift into
        // accidentally pairing across the impossible ordering.
        let events = events_of(&[
            line(
                0,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":5,"result":{"tools":[]}}"#,
            ),
            line(
                1,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        assert_eq!(
            context.exchanges().count(),
            0,
            "a response cannot pair with a request that appears after it"
        );
    }

    #[test]
    fn server_initiated_requests_pair_with_client_responses() {
        let events = events_of(&[
            line(
                0,
                "server-to-client",
                r#"{"jsonrpc":"2.0","id":"s1","method":"sampling/createMessage","params":{}}"#,
            ),
            line(
                1,
                "client-to-server",
                r#"{"jsonrpc":"2.0","id":"s1","result":{"role":"assistant"}}"#,
            ),
        ]);
        let context = TraceContext::new(&events);
        let exchanges: Vec<_> = context.exchanges().collect();
        assert_eq!(exchanges.len(), 1);
        assert_eq!(exchanges[0].method, "sampling/createMessage");
    }
}
