// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the JSON-RPC envelope and correlation checks.

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};
use crate::report::Finding;
use mcp_conformance_core::trace::TraceEvent;

fn run_check(check_id: &str, trace: &str) -> Vec<Finding> {
    let events: Vec<TraceEvent> = parse_trace(trace, &Limits::default()).unwrap();
    let context = TraceContext::new(&events);
    checks::find(check_id).unwrap().run(&context).findings
}

const INIT: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#;

#[test]
fn a_malformed_response_opens_no_request_to_answer() {
    // `method` is not a string, so the message is unclassifiable; it also
    // carries `result`, so it is a malformed response, not a request whose
    // id can be read. A client response with the same id answers nothing.
    let trace = r#"{"seq":0,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":5,"id":7,"result":{}}}
{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":7,"result":{}}}"#;
    let findings = run_check("base.result-id-matches", trace);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].seq, Some(1));
}

#[test]
fn a_response_shaped_message_is_judged_on_its_result_member_alone() {
    // The two cases `classify` can hand this check, and the reason it only
    // asks about `result`: an id-bearing, method-less message that it
    // called `Invalid` carries both members or neither.
    let neither = r#"{"seq":0,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1}}"#;
    let findings = run_check("base.result-field", neither);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].detail.contains("no result field"),
        "{findings:?}"
    );

    // Both: ambiguous, and BASE-006's business rather than this clause's —
    // whatever else is wrong with it, a `result` member is present.
    let both = r#"{"seq":0,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"x"}}}"#;
    assert_eq!(run_check("base.result-field", both), Vec::<Finding>::new());

    // A well-formed result classifies as `Result` and never reaches here.
    let clean = r#"{"seq":0,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#;
    assert_eq!(run_check("base.result-field", clean), Vec::<Finding>::new());
}

#[test]
fn result_response_with_null_id_gets_the_null_detail() {
    // A null-id result is its own finding, distinct from "no outstanding request".
    let trace = format!(
        "{INIT}\n{}",
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":null,"result":{}}}"#
    );
    let findings = run_check("base.result-id-matches", &trace);
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0].detail.contains("null id"),
        "{}",
        findings[0].detail
    );
}

#[test]
fn error_message_member_type_is_named_precisely() {
    // -5 is i64-but-not-u64: the finding must call it an integer, which pins the
    // is_i64 || is_u64 disjunction in type_name.
    let trace = format!(
        "{INIT}\n{}",
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":-5}}}"#
    );
    let findings = run_check("base.error-shape", &trace);
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0]
            .detail
            .contains("is an integer, expected a string"),
        "{}",
        findings[0].detail
    );
}

#[test]
fn u64_only_request_ids_are_valid_integers() {
    // u64::MAX is not representable as i64; it must still count as an integer id.
    let trace = format!(
        "{INIT}\n{}",
        r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":18446744073709551615,"method":"tools/list"}}"#
    );
    assert_eq!(
        run_check("base.request-id-type", &trace),
        Vec::<Finding>::new()
    );
}

/// A request id=2 answered by both an error and a result. The SECOND answer
/// has no outstanding request and must be flagged by its own flavor's check;
/// the cross-flavor consume is what makes that true (without it both checks
/// saw a clean 1:1 and the double-answer slipped through).
const REQUEST: &str = r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#;
const RESULT_2: &str = r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{}}}"#;
const ERROR_2: &str = r#"{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"x"}}}"#;

#[test]
fn error_then_result_flags_the_second_answer_as_a_result() {
    // error (seq2) then result (seq3): the result is the double-answer, so
    // BASE-004 flags it and BASE-009 stays silent (the error was valid).
    let trace = format!("{INIT}\n{REQUEST}\n{ERROR_2}\n{RESULT_2}");
    let results = run_check("base.result-id-matches", &trace);
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].seq, Some(3));
    assert!(
        results[0].detail.contains("already answered"),
        "{results:?}"
    );
    assert!(
        run_check("base.error-id-matches", &trace).is_empty(),
        "the error was the legitimate first answer"
    );
}

#[test]
fn result_then_error_flags_the_second_answer_as_an_error() {
    // Reverse order, so the fix cannot be order-specific: result (seq2) then
    // error (seq3) makes the error the double-answer.
    let result_seq2 = RESULT_2.replace("\"seq\":3", "\"seq\":2");
    let error_seq3 = ERROR_2.replace("\"seq\":2", "\"seq\":3");
    let trace = format!("{INIT}\n{REQUEST}\n{result_seq2}\n{error_seq3}");
    let errors = run_check("base.error-id-matches", &trace);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].seq, Some(3));
    assert!(errors[0].detail.contains("already answered"), "{errors:?}");
    assert!(
        run_check("base.result-id-matches", &trace).is_empty(),
        "the result was the legitimate first answer"
    );
}

#[test]
fn single_flavor_answer_is_not_flagged_by_the_other_pass() {
    // Guard against a cross-flavor consume that over-fires: a request
    // answered once by a result must leave BOTH passes clean.
    let trace = format!(
        "{INIT}\n{REQUEST}\n{}",
        RESULT_2.replace("\"seq\":3", "\"seq\":2")
    );
    assert_eq!(
        run_check("base.result-id-matches", &trace),
        Vec::<Finding>::new()
    );
    assert_eq!(
        run_check("base.error-id-matches", &trace),
        Vec::<Finding>::new()
    );
}

#[test]
fn an_error_answering_a_malformed_request_with_a_readable_id_is_correlated() {
    // The client sends malformed requests whose ids are still readable; the
    // server answers each with -32600 echoing the id, as BASE-009 requires.
    // The null-id escape hatch is only for an id that could not be read.
    let trace = format!(
        "{INIT}\n{}\n{}\n{}\n{}",
        r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":7,"method":5}}"#,
        r#"{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":7,"error":{"code":-32600,"message":"Invalid Request"}}}"#,
        r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":"x","params":{}}}"#,
        r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":"x","error":{"code":-32600,"message":"Invalid Request"}}}"#,
    );
    assert_eq!(
        run_check("base.error-id-matches", &trace),
        Vec::<Finding>::new()
    );

    // An error answering an id nobody sent is still a finding.
    let unsolicited = trace.replace(r#""id":"x","error""#, r#""id":"y","error""#);
    assert_eq!(run_check("base.error-id-matches", &unsolicited).len(), 1);
}

#[test]
fn a_null_id_error_the_framing_ties_is_judged_against_its_request() {
    // The null-id escape hatch is for an id that "could not be read". On
    // HTTP the status ties the error to its POST, whose request the trace
    // holds — and whose id it can read.
    let http = |seq: u64, direction: &str, rest: &str| {
        format!(r#"{{"seq":{seq},"direction":"{direction}","transport":"streamable-http",{rest}}}"#)
    };
    let refused = |id: &str| {
        [
                http(0, "client-to-server", r#""kind":"http","method":"POST""#),
                http(
                    1,
                    "client-to-server",
                    &format!(r#""kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#),
                ),
                http(2, "server-to-client", r#""kind":"http","status":400"#),
                http(
                    3,
                    "server-to-client",
                    r#""kind":"message","payload":{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"x"}}"#,
                ),
            ]
            .join("\n")
    };
    let findings = run_check("base.error-id-matches", &refused("1"));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].seq, Some(3));
    // An object is no id at all; that one could not be read.
    assert_eq!(
        run_check("base.error-id-matches", &refused("{}")),
        Vec::<Finding>::new()
    );
    // The null answer closed the request, so a second answer is unsolicited.
    let twice = format!(
        "{}\n{}",
        refused("1"),
        http(
            4,
            "server-to-client",
            r#""kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}"#
        )
    );
    assert_eq!(run_check("base.result-id-matches", &twice).len(), 1);
}
