// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use super::super::testkit::{Session, ping, pong};

const TERMINATED: &str = "transport.terminated-session-404";
const MISSING: &str = "transport.missing-session-id-400";

/// Headers naming session `S`.
const S: &str = r#"{"mcp-session-id":"S"}"#;
/// Headers naming no session.
const NONE: &str = "{}";
const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{}}"#;

/// A session `S` in use: one `ping` served under it.
fn live() -> Session {
    Session::default().post(S, &ping(1), 200).answers(&pong(1))
}

/// [`live`], then `S` terminated by a `DELETE` answered `status`.
fn ended(status: u16) -> Session {
    live().bare("DELETE", S, status)
}

#[test]
fn a_request_on_a_terminated_session_answered_404_passes() {
    for delete in [200, 204] {
        let trace = ended(delete).post(S, &ping(2), 404);
        assert_eq!(trace.clean(TERMINATED), 1, "DELETE answered {delete}");
    }
    // A GET names the session as much as a POST does.
    assert_eq!(ended(200).bare("GET", S, 404).clean(TERMINATED), 1);
}

#[test]
fn a_terminated_session_that_is_served_is_a_violation() {
    let finding = ended(200).post(S, &ping(2), 200).one(TERMINATED);
    assert!(finding.contains("request at seq 6"), "{finding}");
    assert!(finding.contains(r#"MCP-Session-Id "S""#), "{finding}");
    assert!(finding.contains("answered HTTP 200"), "{finding}");
    // 2xx is a range, and its edges are served too.
    for served in [200, 204, 299] {
        ended(204).bare("GET", S, served).one(TERMINATED);
    }
}

#[test]
fn without_a_recorded_termination_nothing_is_judged() {
    // No DELETE at all: a 404 or a 200 says nothing about this clause.
    assert_eq!(live().post(S, &ping(2), 404).clean(TERMINATED), 0);
    // A DELETE the server refused, or only accepted for later, ended nothing
    // the trace can rely on.
    for delete in [202, 405, 404, 500] {
        let trace = ended(delete).post(S, &ping(2), 200);
        assert_eq!(trace.clean(TERMINATED), 0, "DELETE answered {delete}");
    }
    // A DELETE naming no session terminated none.
    let trace = live().bare("DELETE", NONE, 200).post(S, &ping(2), 200);
    assert_eq!(trace.clean(TERMINATED), 0);
}

#[test]
fn only_requests_naming_the_terminated_session_are_judged() {
    let other = r#"{"mcp-session-id":"T"}"#;
    assert_eq!(ended(200).post(other, &ping(2), 200).clean(TERMINATED), 0);
    assert_eq!(ended(200).post(NONE, &ping(2), 200).clean(TERMINATED), 0);
    // An `initialize` is the start of another session, whatever header it has.
    assert_eq!(ended(200).post(S, INITIALIZE, 200).clean(TERMINATED), 0);
    // A POST whose body the trace does not hold is not known not to be one.
    assert_eq!(ended(200).bare("POST", S, 200).clean(TERMINATED), 0);
    // A second DELETE, answered idempotently, is not judged.
    assert_eq!(ended(200).bare("DELETE", S, 200).clean(TERMINATED), 0);
    // Nor is a verb the transport does not define.
    assert_eq!(ended(200).bare("PUT", S, 200).clean(TERMINATED), 0);
}

#[test]
fn a_session_id_issued_again_is_live_again() {
    // Reissued in answer to a request that did not name it.
    let reissued = ended(200)
        .request("POST", NONE)
        .sends(INITIALIZE)
        .status(200, S)
        .post(S, &ping(2), 200);
    assert_eq!(reissued.clean(TERMINATED), 0);
    // Reissued on a status no recorded request awaited.
    let unattributed = ended(200).status(200, S).post(S, &ping(2), 200);
    assert_eq!(unattributed.clean(TERMINATED), 0);
    // An echo — the header on the answer to a request that named it — issues
    // nothing: both requests are still on a terminated session.
    let echoed = ended(200)
        .request("POST", S)
        .sends(&ping(2))
        .status(200, S)
        .post(S, &ping(3), 200);
    let (findings, subjects) = echoed.run(TERMINATED);
    assert_eq!((findings.len(), subjects), (2, 2), "{findings:?}");
}

#[test]
fn another_refusal_is_a_violation_only_beside_the_same_request_served() {
    // `live()` holds the twin: the same ping, same headers, answered 200.
    for refused in [400, 410, 499] {
        let finding = ended(200).post(S, &ping(2), refused).one(TERMINATED);
        assert!(
            finding.contains(&format!("answered HTTP {refused}")),
            "{finding}"
        );
    }
    // Statuses with a cause the recording does not carry are never judged.
    for unexplained in [401, 403, 408, 429, 500, 302] {
        let trace = ended(200).post(S, &ping(2), unexplained);
        assert_eq!(trace.clean(TERMINATED), 0, "answered {unexplained}");
    }
    // A 404 is the conforming answer with or without a twin.
    let not_found = ended(200).post(S, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, 404);
    assert_eq!(not_found.clean(TERMINATED), 1);
    // Without the twin, a 400 may be the request's own fault.
    let no_twin = ended(200).post(S, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, 400);
    assert_eq!(no_twin.clean(TERMINATED), 0);
}

/// A `ping` served under `S`, then the same `ping` with no session answered
/// `status`.
fn without_session(status: u16) -> Session {
    live().post(NONE, &ping(2), status)
}

#[test]
fn a_session_less_request_refused_400_passes() {
    assert_eq!(without_session(400).clean(MISSING), 1);
}

#[test]
fn a_session_less_request_refused_otherwise_is_reported() {
    let finding = without_session(404).one(MISSING);
    assert!(finding.contains("request at seq 4"), "{finding}");
    assert!(finding.contains("refused HTTP 404"), "{finding}");
    assert!(
        finding.contains("answered HTTP 200 at seq 2"),
        "names the twin: {finding}"
    );
    without_session(405).one(MISSING);
}

#[test]
fn a_server_that_serves_the_session_less_request_is_not_a_subject() {
    assert_eq!(without_session(200).clean(MISSING), 0);
    // Nor is one that refuses it for a reason the recording does not carry.
    for unexplained in [401, 403, 408, 429, 500] {
        assert_eq!(
            without_session(unexplained).clean(MISSING),
            0,
            "answered {unexplained}"
        );
    }
}

#[test]
fn the_refusal_is_judged_only_beside_its_served_twin() {
    // No twin at all.
    let alone = Session::default().post(NONE, &ping(1), 404);
    assert_eq!(alone.clean(MISSING), 0);
    // A twin that was itself refused shows nothing.
    let both_refused = Session::default()
        .post(S, &ping(1), 404)
        .post(NONE, &ping(2), 404);
    assert_eq!(both_refused.clean(MISSING), 0);
    // A different message is not a twin.
    let other_message = live().post(
        NONE,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        404,
    );
    assert_eq!(other_message.clean(MISSING), 0);
    // Nor a different verb, even where neither carries a body.
    let bodiless = Session::default()
        .bare("PUT", S, 200)
        .bare("GET", NONE, 404);
    assert_eq!(bodiless.clean(MISSING), 0);
    // Nor a different verb.
    let other_verb = live().bare("GET", NONE, 404);
    assert_eq!(other_verb.clean(MISSING), 0);
    // Nor a request differing in a second header.
    let other_header = live().post(r#"{"accept":"text/plain"}"#, &ping(2), 404);
    assert_eq!(other_header.clean(MISSING), 0);
}

#[test]
fn a_request_that_names_a_session_or_initializes_is_outside_the_clause() {
    // Two sessions answered differently: the refused request did carry an id.
    let named = live().post(r#"{"mcp-session-id":"T"}"#, &ping(2), 404);
    assert_eq!(named.clean(MISSING), 0);
    // "(other than initialization)".
    let initialize = Session::default()
        .post(S, INITIALIZE, 200)
        .post(NONE, INITIALIZE, 404);
    assert_eq!(initialize.clean(MISSING), 0);
}

#[test]
fn bodiless_requests_have_twins_and_unpaired_posts_do_not() {
    // GET with a session served, without one refused.
    let get = Session::default()
        .bare("GET", S, 200)
        .bare("GET", NONE, 404);
    get.one(MISSING);
    let delete = Session::default()
        .bare("DELETE", S, 200)
        .bare("DELETE", NONE, 400);
    assert_eq!(delete.clean(MISSING), 1);
    // Two POSTs whose bodies the trace does not hold are not known to agree.
    let unpaired = Session::default()
        .bare("POST", S, 200)
        .bare("POST", NONE, 404);
    assert_eq!(unpaired.clean(MISSING), 0);
    // And a body on one side only never agrees.
    let half = Session::default()
        .bare("POST", S, 200)
        .post(NONE, &ping(2), 404);
    assert_eq!(half.clean(MISSING), 0);
}
