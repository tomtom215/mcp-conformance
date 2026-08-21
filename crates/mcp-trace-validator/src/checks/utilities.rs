// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` server-utilities requirements: logging (`LOG-*`),
//! completion (`COMP-*`), and pagination (`PAGE-*`).

use std::collections::BTreeSet;

use serde_json::Value;

use super::FindingSink;
use super::support::{Declaration, server_capability};
use crate::context::TraceContext;
use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;

/// `LOG-001`: "Servers that emit log message notifications MUST declare the `logging`
/// capability:" — emission is directly observable.
pub(super) fn logging_capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // The subject is an emitted log notification, declaration or not; a session
    // in which the server never logged leaves this clause untested.
    let declared = match server_capability(context, &["logging"]) {
        Declaration::Declared => true,
        Declaration::Withheld => false,
        // Nothing in this trace could have declared anything, so it shows
        // neither compliance nor violation: abstain before counting a subject.
        Declaration::Unknowable => return,
    };
    for (event, kind, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        if matches!(kind, MessageKind::Notification { method } if *method == "notifications/message")
        {
            sink.examined();
            if !declared {
                sink.push(
                    Some(event.seq),
                    "server emitted a log message notification without declaring the logging capability"
                        .to_owned(),
                );
            }
        }
    }
}

/// `COMP-001`: "Servers that support completions MUST declare the `completions`
/// capability:" — successfully answering `completion/complete` is the observable form
/// of support.
pub(super) fn completion_capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // The subject is an answered completion, declaration or not; a session that
    // never asked for one leaves this clause untested.
    let declared = match server_capability(context, &["completions"]) {
        Declaration::Declared => true,
        Declaration::Withheld => false,
        // Nothing in this trace could have declared anything, so it shows
        // neither compliance nor violation: abstain before counting a subject.
        Declaration::Unknowable => return,
    };
    for exchange in context.exchanges_for("completion/complete") {
        if exchange.result.is_some() {
            sink.examined();
            if !declared {
                sink.push(
                    Some(exchange.response.seq),
                    "server answered completion/complete without declaring the completions capability"
                        .to_owned(),
                );
            }
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
            && PAGINATED_METHODS.contains(method)
        {
            check_cursor_provenance(event, method, &issued, sink);
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
    // The subject is a *continuation* request: a first page carries no cursor
    // and so puts no opacity claim to the test.
    sink.examined();
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

/// JSON-RPC `Invalid params`, which an invalid cursor must draw.
const INVALID_PARAMS: i64 = -32602;

/// `PAGE-003` / `PAGE-011`: an invalid cursor draws `-32602`.
///
/// "Invalid" is witnessed the only way a recording can witness it: the client
/// presented a cursor that this session never issued as a `nextCursor` for that
/// method. A cursor that *was* issued may still have expired, and a trace cannot
/// tell — so those are not judged, and the check abstains rather than guessing.
///
/// That narrowing is the whole point, and it is why `2025-11-25` carried an
/// exclusion here until 2026-08-21: *"whether a cursor is invalid is
/// server-internal knowledge; a trace cannot distinguish a server accepting a
/// stale-but-valid cursor from one silently tolerating an invalid one."* True of
/// the general case, and it reads as a verdict on the clause. The narrow case
/// the sentence itself excludes — a cursor with no issuance anywhere in the
/// session — is decidable from the recording alone, and the clause is judged on
/// exactly that. The exclusion was written before the witness was found and
/// nothing re-read it; `corpus/violations/page-002-cursor-never-issued.jsonl`
/// had been sitting in the corpus, fabricated cursor answered with a result,
/// the whole time.
///
/// Where this fires, the opacity clause usually fires too, and that is not
/// double reporting: the client fabricated the cursor (its defect) and the
/// server then honoured it instead of rejecting it (the server's). Only the
/// second is this clause's.
pub(super) fn invalid_cursor_rejected(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut issued: BTreeSet<(&str, &str)> = BTreeSet::new();
    // Issuances take effect after the result that carried them, so a cursor is
    // only "known" to requests that follow it — walking exchanges in order keeps
    // a server from being excused by a cursor it had not yet handed out.
    let mut exchanges: Vec<_> = context.exchanges().collect();
    exchanges.sort_by_key(|exchange| exchange.request.seq);
    for exchange in exchanges {
        if !PAGINATED_METHODS.contains(&exchange.method) {
            continue;
        }
        let presented = exchange
            .params
            .and_then(|params| params.get("cursor"))
            .and_then(Value::as_str);
        if let Some(cursor) = presented
            && !issued.contains(&(exchange.method, cursor))
        {
            // The subject is a request presenting a cursor this session never
            // issued: no such request, and the clause is untested here.
            sink.examined();
            let code = exchange
                .response
                .message_payload()
                .and_then(|payload| payload.get("error"))
                .and_then(|error| error.get("code"))
                .and_then(Value::as_i64);
            if code != Some(INVALID_PARAMS) {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "`{}` presented the cursor {cursor:?}, which this session never issued, \
                         and the server answered with {} rather than {INVALID_PARAMS}",
                        exchange.method,
                        code.map_or_else(|| "a result".to_owned(), |code| format!("error {code}"))
                    ),
                );
            }
        }
        if let Some(next) = exchange
            .result
            .and_then(|result| result.get("nextCursor"))
            .and_then(Value::as_str)
        {
            issued.insert((exchange.method, next));
        }
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
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    const HANDSHAKE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;

    // --- PAGE-003 / PAGE-011: the invalid-cursor rejection ---------------
    //
    // The check treats "never issued in this session" as the witness for
    // "invalid", so the cases that keep it honest are a cursor issued
    // *earlier* (valid, not reported) and one issued for a different method
    // (not this method's cursor).

    const INVALID_CURSOR: &str = "pagination.invalid-cursor-rejected";

    /// A list request for `method`, presenting `cursor` when given.
    fn list(seq: u64, id: u64, method: &str, cursor: Option<&str>) -> String {
        let params = cursor.map_or_else(String::new, |cursor| {
            format!(r#","params":{{"cursor":"{cursor}"}}"#)
        });
        format!(
            r#"{{"seq":{seq},"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"method":"{method}"{params}}}}}"#
        )
    }

    /// A page result, issuing `next` when given.
    fn page(seq: u64, id: u64, next: Option<&str>) -> String {
        let cursor = next.map_or_else(String::new, |next| format!(r#","nextCursor":"{next}""#));
        format!(
            r#"{{"seq":{seq},"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"result":{{"tools":[]{cursor}}}}}}}"#
        )
    }

    /// An error answer carrying `code`.
    fn error(seq: u64, id: u64, code: i64) -> String {
        format!(
            r#"{{"seq":{seq},"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"error":{{"code":{code},"message":"no"}}}}}}"#
        )
    }

    fn session(body: &[String]) -> String {
        format!("{HANDSHAKE}\n{}", body.join("\n"))
    }

    #[test]
    fn an_unissued_cursor_answered_with_a_result_is_reported() {
        let trace = session(&[list(3, 2, "tools/list", Some("made-up")), page(4, 2, None)]);
        let findings = findings_for(INVALID_CURSOR, &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("made-up"), "{findings:?}");
    }

    #[test]
    fn rejecting_it_with_invalid_params_conforms() {
        let trace = session(&[
            list(3, 2, "tools/list", Some("made-up")),
            error(4, 2, -32602),
        ]);
        assert!(findings_for(INVALID_CURSOR, &trace).is_empty());
    }

    #[test]
    fn some_other_error_is_not_the_required_rejection() {
        let trace = session(&[
            list(3, 2, "tools/list", Some("made-up")),
            error(4, 2, -32603),
        ]);
        let findings = findings_for(INVALID_CURSOR, &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("-32603"), "{findings:?}");
    }

    #[test]
    fn a_cursor_the_server_issued_is_valid() {
        let trace = session(&[
            list(3, 2, "tools/list", None),
            page(4, 2, Some("page2")),
            list(5, 3, "tools/list", Some("page2")),
            page(6, 3, None),
        ]);
        assert!(findings_for(INVALID_CURSOR, &trace).is_empty());
    }

    #[test]
    fn a_cursor_issued_for_another_method_is_not_this_ones() {
        let trace = session(&[
            list(3, 2, "prompts/list", None),
            page(4, 2, Some("page2")),
            list(5, 3, "tools/list", Some("page2")),
            page(6, 3, None),
        ]);
        assert_eq!(findings_for(INVALID_CURSOR, &trace).len(), 1);
    }

    #[test]
    fn a_cursor_used_before_it_was_issued_is_still_unissued() {
        // Order matters: the server cannot be excused by a cursor it handed
        // out afterwards.
        let trace = session(&[
            list(3, 2, "tools/list", Some("page2")),
            page(4, 2, Some("page2")),
        ]);
        assert_eq!(findings_for(INVALID_CURSOR, &trace).len(), 1);
    }

    #[test]
    fn a_list_request_with_no_cursor_is_not_judged() {
        let trace = session(&[list(3, 2, "tools/list", None), page(4, 2, None)]);
        assert!(findings_for(INVALID_CURSOR, &trace).is_empty());
    }

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
