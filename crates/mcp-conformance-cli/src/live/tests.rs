// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mcp_conformance_core::trace::Direction;

use super::*;

const INITIALIZE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#;
const INITIALIZED: &str = r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}}"#;
const NOTIFIED: &str = r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;

fn at(second: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000 + second)
}

fn line(seq: u64, direction: &str, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
    )
}

/// Pushes `lines`, judging whenever a line is due; returns each pass's new
/// findings' requirement IDs, by the index of the line that triggered it.
fn replay(live: &mut Live, lines: &[String]) -> Vec<(usize, Vec<String>)> {
    let mut passes = Vec::new();
    for (index, text) in lines.iter().enumerate() {
        let pushed = live.push(text.as_bytes(), at(index as u64));
        if pushed.due {
            let pass = live.judge();
            passes.push((
                index,
                pass.new
                    .into_iter()
                    .map(|finding| finding.requirement)
                    .collect(),
            ));
        }
    }
    passes
}

fn handshake() -> Vec<String> {
    vec![
        INITIALIZE.to_owned(),
        INITIALIZED.to_owned(),
        NOTIFIED.to_owned(),
    ]
}

#[test]
fn a_clean_session_reports_nothing_however_its_outcomes_move() {
    // Clauses go from not-observed to pass as the session reaches them: none
    // of that is a finding.
    let mut lines = handshake();
    lines.push(line(
        3,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    ));
    lines.push(line(
        4,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}"#,
    ));
    let mut live = Live::new().unwrap();
    let passes = replay(&mut live, &lines);
    assert_eq!(
        passes.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        [1, 2, 4],
        "a pass after each response and notification, none after a request"
    );
    assert!(passes.iter().all(|(_, new)| new.is_empty()), "{passes:?}");
    let first = live.judge();
    assert!(first.totals[0].1.pass > 0);
    let ended = live.finish(at(9));
    assert!(ended.new.is_empty() && ended.withdrawn.is_empty());
    assert!(ended.unanswered.is_empty(), "{:?}", ended.unanswered);
    assert!(ended.judgment.is_ok());
}

#[test]
fn a_finding_a_later_message_retracts_is_never_reported() {
    // Right after the initialize response, LIFE-003 (no initialized
    // notification yet) fails on the prefix; here the server asks the client
    // something first, so the notification retracting it comes two exchanges
    // later.
    let mut live = Live::new().unwrap();
    live.push(INITIALIZE.as_bytes(), at(0));
    live.push(INITIALIZED.as_bytes(), at(1));
    let prefix = judge::judge(&live.document).unwrap();
    assert!(
        prefix.reports[0]
            .requirements
            .iter()
            .any(|row| row.id == "LIFE-003" && row.outcome == Outcome::Fail),
        "the prefix alone does fail it"
    );
    let new = live.judge().new;
    assert!(new.is_empty(), "{new:?}");
    let ping = line(
        2,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":"s1","method":"ping"}"#,
    );
    let answer = line(
        3,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":"s1","result":{}}"#,
    );
    let notified = NOTIFIED.replace(r#""seq":2"#, r#""seq":4"#);
    for (second, text) in [ping, answer, notified].iter().enumerate() {
        live.push(text.as_bytes(), at(2 + second as u64));
        let new = live.judge().new;
        assert!(new.is_empty(), "{new:?}");
    }
    let ended = live.finish(at(9));
    assert!(ended.new.is_empty() && ended.withdrawn.is_empty());
}

#[test]
fn an_eventual_finding_the_session_never_retracts_is_reported_at_the_end() {
    let mut live = Live::new().unwrap();
    live.push(INITIALIZE.as_bytes(), at(0));
    live.push(INITIALIZED.as_bytes(), at(1));
    let new = live.judge().new;
    assert!(new.is_empty(), "{new:?}");
    let ended = live.finish(at(2));
    let ids: Vec<&str> = ended.new.iter().map(|f| f.requirement.as_str()).collect();
    assert_eq!(ids, ["LIFE-003"]);
    assert!(ended.withdrawn.is_empty(), "{:?}", ended.withdrawn);
}

#[test]
fn a_violation_is_reported_once_by_the_pass_its_exchange_completes() {
    let mut lines = handshake();
    lines.push(line(
        3,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    ));
    // TOOL-006: a name with characters outside the allowed set.
    lines.push(line(
        4,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"weather lookup,v2!","inputSchema":{"type":"object"}}]}}"#,
    ));
    lines.push(line(
        5,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
    ));
    lines.push(line(
        6,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":3,"result":{}}"#,
    ));
    let mut live = Live::new().unwrap();
    let passes = replay(&mut live, &lines);
    let reported: Vec<(usize, &str)> = passes
        .iter()
        .flat_map(|(index, new)| new.iter().map(move |id| (*index, id.as_str())))
        .collect();
    assert!(
        reported.iter().all(|(index, _)| *index == 4),
        "reported by the pass of the violating response, and only then: {reported:?}"
    );
    assert!(
        reported.iter().any(|(_, id)| *id == "TOOL-006"),
        "{reported:?}"
    );
    // A later pass sees it again and says nothing.
    let new = live.judge().new;
    assert!(new.is_empty(), "{new:?}");
    let ended = live.finish(at(9));
    assert!(ended.new.is_empty(), "{:?}", ended.new);
    assert!(ended.withdrawn.is_empty(), "{:?}", ended.withdrawn);
}

#[test]
fn a_finding_only_the_whole_trace_shows_is_reported_at_the_end() {
    // A request reusing an answered id, never answered itself: requests are not
    // judged on arrival, so BASE-003 is the final judgment's to report.
    let mut lines = handshake();
    lines.push(line(
        3,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    ));
    lines.push(line(
        4,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":2,"result":{}}"#,
    ));
    lines.push(line(
        5,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    ));
    let mut live = Live::new().unwrap();
    let passes = replay(&mut live, &lines);
    assert!(passes.iter().all(|(_, new)| new.is_empty()), "{passes:?}");
    let ended = live.finish(at(9));
    let ids: Vec<&str> = ended.new.iter().map(|f| f.requirement.as_str()).collect();
    assert!(ids.contains(&"BASE-003"), "{ids:?}");
    assert_eq!(ended.unanswered.len(), 1);
    assert_eq!(ended.unanswered[0].request_seq, 5);
    assert_eq!(ended.unanswered[0].ending, Ending::Unanswered);
    assert_eq!(ended.unanswered[0].ended, at(9));
}

#[test]
fn a_finding_the_final_judgment_does_not_show_is_withdrawn() {
    let finding = |seq| LiveFinding {
        revision: "2025-11-25".to_owned(),
        requirement: "BASE-003".to_owned(),
        level: "MUST NOT".to_owned(),
        outcome: Outcome::Fail,
        check: "base.request-id-unique".to_owned(),
        seq: Some(seq),
        detail: "reused".to_owned(),
        quote: None,
        url: None,
    };
    let reported = [finding(3), finding(5)];
    let withdrawn = super::withdrawn(&reported, &[finding(5)]);
    assert_eq!(withdrawn, [finding(3)]);
    assert_eq!(super::withdrawn(&reported, &reported), []);
}

#[test]
fn exchanges_pair_by_direction_and_id_and_carry_what_spans_need() {
    let mut live = Live::new().unwrap();
    assert!(!live.push(INITIALIZE.as_bytes(), at(0)).due);
    let pushed = live.push(INITIALIZED.as_bytes(), at(2));
    assert!(pushed.due);
    let [initialize] = pushed.completed.as_slice() else {
        panic!("{:?}", pushed.completed)
    };
    assert_eq!(initialize.method, "initialize");
    assert_eq!(initialize.id.as_deref(), Some("1"));
    assert_eq!((initialize.started, initialize.ended), (at(0), at(2)));
    assert_eq!(
        (initialize.request_seq, initialize.response_seq),
        (0, Some(1))
    );
    assert_eq!(initialize.protocol_version.as_deref(), Some("2025-11-25"));
    assert!(initialize.holds(0) && initialize.holds(1) && !initialize.holds(2));

    let notified = live.push(NOTIFIED.as_bytes(), at(3));
    assert_eq!(notified.completed[0].ending, Ending::Notification);
    assert_eq!(notified.completed[0].id, None);

    // A server request with the same id as a client one is its own exchange.
    let call = line(
        3,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"echo","arguments":{"secret":"x"}}}"#,
    );
    let asked = line(
        4,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#,
    );
    let answer = line(
        5,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":"a","result":{"isError":true,"content":[]}}"#,
    );
    assert!(!live.push(call.as_bytes(), at(4)).due);
    assert!(!live.push(asked.as_bytes(), at(5)).due);
    let answered = live.push(answer.as_bytes(), at(6));
    let [tool] = answered.completed.as_slice() else {
        panic!("{:?}", answered.completed)
    };
    assert_eq!(tool.method, "tools/call");
    assert_eq!(tool.tool.as_deref(), Some("echo"));
    assert_eq!(tool.id.as_deref(), Some("\"a\""));
    assert_eq!(
        tool.ending,
        Ending::Answered {
            error: None,
            tool_error: true
        }
    );
    let stray = line(
        6,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":99,"result":{}}"#,
    );
    let pushed = live.push(stray.as_bytes(), at(7));
    assert!(
        pushed.due && pushed.completed.is_empty(),
        "judged, but no exchange"
    );
    let not_rpc = line(7, "server-to-client", r#""not json-rpc""#);
    assert!(live.push(not_rpc.as_bytes(), at(8)).due);
    let lifecycle = r#"{"seq":8,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-close"}"#;
    assert!(!live.push(lifecycle.as_bytes(), at(9)).due);
    assert_eq!(live.exchanges(), 3);
    assert_eq!(live.events(), 9);
    let ended = live.finish(at(10));
    let [ping] = ended.unanswered.as_slice() else {
        panic!("{:?}", ended.unanswered)
    };
    assert_eq!(
        (ping.method.as_str(), ping.initiator),
        ("ping", Direction::ServerToClient)
    );
}

#[test]
fn http_exchanges_carry_the_session_id_and_an_error_response_its_code() {
    let http = |seq: u64, headers: &str| {
        format!(
            r#"{{"seq":{seq},"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200,"headers":{headers}}}"#
        )
    };
    let message = |seq: u64, direction: &str, payload: &str| {
        format!(
            r#"{{"seq":{seq},"direction":"{direction}","transport":"streamable-http","kind":"message","payload":{payload}}}"#
        )
    };
    let mut live = Live::new().unwrap();
    live.push(
        message(
            0,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"file:///a"}}"#,
        )
        .as_bytes(),
        at(0),
    );
    live.push(
        http(
            1,
            r#"{"Mcp-Session-Id":"abc","mcp-protocol-version":"2025-11-25"}"#,
        )
        .as_bytes(),
        at(1),
    );
    let pushed = live.push(
        message(
            2,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32002,"message":"Resource not found"}}"#,
        )
        .as_bytes(),
        at(2),
    );
    let [read] = pushed.completed.as_slice() else {
        panic!("{:?}", pushed.completed)
    };
    assert_eq!(read.resource_uri.as_deref(), Some("file:///a"));
    assert_eq!(read.session_id.as_deref(), Some("abc"));
    assert_eq!(read.protocol_version.as_deref(), Some("2025-11-25"));
    assert_eq!(
        read.ending,
        Ending::Answered {
            error: Some((
                Some("-32002".to_owned()),
                Some("Resource not found".to_owned())
            )),
            tool_error: false
        }
    );
}

#[test]
fn a_line_that_is_not_an_event_is_kept_for_the_final_judgment_only() {
    let mut live = Live::new().unwrap();
    assert!(!live.push(b"not json", at(0)).due);
    assert!(!live.push(&[0xff, 0xfe], at(1)).due);
    assert_eq!(live.events(), 0);
    let ended = live.finish(at(2));
    assert!(matches!(ended.judgment, Err(JudgeError::Malformed(_))));
}

#[test]
fn the_revision_is_the_one_initialize_declares_until_its_response_negotiates_another() {
    let mut live = Live::new().unwrap();
    // The revision each exchange a line completes carries.
    let mut revisions = |seq: u64, direction: &str, payload: &str| -> Vec<Option<String>> {
        live.push(line(seq, direction, payload).as_bytes(), at(0))
            .completed
            .into_iter()
            .map(|exchange| exchange.protocol_version)
            .collect()
    };
    let none: Vec<Option<String>> = Vec::new();
    // Only initialize's own parameter declares a revision.
    assert_eq!(
        revisions(
            0,
            "client-to-server",
            r#"{"jsonrpc":"2.0","method":"notifications/x","params":{"protocolVersion":"2024-11-05"}}"#,
        ),
        [None]
    );
    assert_eq!(
        revisions(
            1,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        ),
        none
    );
    let declared = || Some("2025-06-18".to_owned());
    assert_eq!(
        revisions(
            2,
            "client-to-server",
            r#"{"jsonrpc":"2.0","method":"notifications/y"}"#
        ),
        [declared()]
    );
    // A result naming a revision negotiates only when it answers initialize.
    assert_eq!(
        revisions(
            3,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#
        ),
        none
    );
    assert_eq!(
        revisions(
            4,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":2,"result":{"protocolVersion":"2024-11-05"}}"#,
        ),
        [declared()]
    );
    assert_eq!(
        revisions(
            5,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26"}}"#,
        ),
        [Some("2025-03-26".to_owned())]
    );
}

#[test]
fn only_a_jsonrpc_member_other_than_2_0_is_kept() {
    let mut live = Live::new().unwrap();
    let mut jsonrpc = |payload: &str| {
        live.push(line(0, "client-to-server", payload).as_bytes(), at(0))
            .completed[0]
            .jsonrpc
            .clone()
    };
    assert_eq!(jsonrpc(r#"{"jsonrpc":"2.0","method":"n"}"#), None);
    assert_eq!(jsonrpc(r#"{"method":"n"}"#), None);
    assert_eq!(
        jsonrpc(r#"{"jsonrpc":"1.0","method":"n"}"#).as_deref(),
        Some("1.0")
    );
    assert_eq!(
        jsonrpc(r#"{"jsonrpc":2,"method":"n"}"#).as_deref(),
        Some("2")
    );
}
