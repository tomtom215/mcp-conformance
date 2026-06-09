// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` server-utilities requirements: logging (`LOG-*`),
//! completion (`COMP-*`), and pagination (`PAGE-*`).

use super::FindingSink;
use super::support::server_capability;
use crate::context::TraceContext;
use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;

/// `LOG-001`: "Servers that emit log message notifications MUST declare the `logging`
/// capability:" — emission is directly observable.
pub(super) fn logging_capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    if server_capability(context, &["logging"]) != Some(false) {
        return;
    }
    for (event, kind, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        if matches!(kind, MessageKind::Notification { method } if *method == "notifications/message")
        {
            sink.push(
                Some(event.seq),
                "server emitted a log message notification without declaring the logging capability"
                    .to_owned(),
            );
        }
    }
}

/// `COMP-001`: "Servers that support completions MUST declare the `completions`
/// capability:" — successfully answering `completion/complete` is the observable form
/// of support.
pub(super) fn completion_capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    if server_capability(context, &["completions"]) != Some(false) {
        return;
    }
    for exchange in context.exchanges_for("completion/complete") {
        if exchange.result.is_some() {
            sink.push(
                Some(exchange.response.seq),
                "server answered completion/complete without declaring the completions capability"
                    .to_owned(),
            );
        }
    }
}

/// The list-style methods whose results may carry a `nextCursor`.
const PAGINATED_METHODS: &[&str] = &[
    "resources/list",
    "resources/templates/list",
    "prompts/list",
    "tools/list",
];

/// `PAGE-002`: clients must treat cursors as opaque tokens. The trace-observable
/// violation is *provenance*: a `cursor` parameter the server never issued as a
/// `nextCursor` for that method earlier in this session is fabricated, modified, or
/// carried over from another session — all three of which the clause forbids.
pub(super) fn cursor_opacity(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // nextCursor issuances, keyed by the seq of the result that carried them.
    let issuances: std::collections::BTreeMap<u64, (&str, &str)> = context
        .exchanges()
        .filter(|exchange| PAGINATED_METHODS.contains(&exchange.method))
        .filter_map(|exchange| {
            let cursor = exchange.result?.get("nextCursor")?.as_str()?;
            Some((exchange.response.seq, (exchange.method, cursor)))
        })
        .collect();

    let mut issued: Vec<(&str, &str)> = Vec::new();
    for (event, kind, _) in context.messages() {
        if let (Direction::ClientToServer, MessageKind::Request { method, .. }) =
            (event.direction, kind)
        {
            if PAGINATED_METHODS.contains(method) {
                check_cursor_provenance(event, method, &issued, sink);
            }
        }
        // Issuances take effect after their event, in trace order.
        if let Some(issuance) = issuances.get(&event.seq) {
            issued.push(*issuance);
        }
    }
}

fn check_cursor_provenance(
    event: &mcp_conformance_core::trace::TraceEvent,
    method: &str,
    issued: &[(&str, &str)],
    sink: &mut FindingSink,
) {
    let cursor = event
        .message_payload()
        .and_then(|payload| payload.get("params"))
        .and_then(|params| params.get("cursor"));
    let Some(cursor) = cursor else { return };
    let Some(cursor) = cursor.as_str() else {
        sink.push(
            Some(event.seq),
            format!("{method} cursor is {cursor}, expected an opaque string token"),
        );
        return;
    };
    if !issued.contains(&(method, cursor)) {
        sink.push(
            Some(event.seq),
            format!(
                "{method} cursor {cursor:?} was never issued as a nextCursor for that method in this session"
            ),
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::checks;
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};

    fn findings_for(check: &str, trace: &str) -> Vec<String> {
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check)
            .unwrap()
            .run(&context)
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    const HANDSHAKE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;

    #[test]
    fn issued_cursors_may_be_replayed_for_the_same_method() {
        let trace = format!(
            "{HANDSHAKE}\n{}\n{}\n{}\n{}",
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#,
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"tools":[],"nextCursor":"abc"}}}"#,
            r#"{"seq":5,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"cursor":"abc"}}}"#,
            r#"{"seq":6,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"result":{"tools":[]}}}"#,
        );
        assert!(findings_for("pagination.cursor-opacity", &trace).is_empty());
    }

    #[test]
    fn cursors_do_not_transfer_between_methods() {
        // A cursor issued for tools/list replayed against prompts/list is misuse.
        let trace = format!(
            "{HANDSHAKE}\n{}\n{}\n{}",
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#,
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"tools":[],"nextCursor":"abc"}}}"#,
            r#"{"seq":5,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"method":"prompts/list","params":{"cursor":"abc"}}}"#,
        );
        let findings = findings_for("pagination.cursor-opacity", &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("prompts/list"), "{findings:?}");
    }

    #[test]
    fn non_string_cursors_are_flagged_as_non_opaque() {
        let trace = format!(
            "{HANDSHAKE}\n{}",
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"cursor":7}}}"#,
        );
        let findings = findings_for("pagination.cursor-opacity", &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].contains("expected an opaque string"),
            "{findings:?}"
        );
    }
}
