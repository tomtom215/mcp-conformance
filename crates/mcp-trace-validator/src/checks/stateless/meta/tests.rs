// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `_meta`-envelope clauses.
//!
//! The trace-context keys have their own module and their own tests; what is
//! here is the protocol's own `_meta` fields — the required envelope, what a
//! missing one must draw, and the HTTP status that answer rides. Capability
//! reliance has its own module too.

use crate::checks::stateless::testkit::{META, client, findings_for, post, server, status, trace};

/// A request whose `_meta` omits `clientCapabilities`, answered by `answer`.
fn incomplete_envelope(answer: &str) -> String {
    trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
        ),
        server(1, answer),
    ])
}

/// A `-32021` whose `data` is `data`.
fn capability_error(data: &str) -> String {
    trace(&[
        client(
            0,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":-32021,"message":"x"{data}}}}}"#),
        ),
    ])
}

/// A `subscriptions/listen` session carrying `notification` on its stream.
fn listening(notification: &str) -> String {
    trace(&[
        client(
            0,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"subscriptions/listen","params":{{{META}}}}}"#
            ),
        ),
        server(1, notification),
    ])
}

#[test]
fn a_malformed_envelope_must_draw_invalid_params() {
    let check = "meta.missing-required-field-rejected";

    // A result, or the wrong error code: both are failures to reject.
    assert_eq!(
        findings_for(
            check,
            &incomplete_envelope(r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#)
        )
        .len(),
        1
    );
    let findings = findings_for(
        check,
        &incomplete_envelope(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"x"}}"#),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("-32603"), "{findings:?}");

    // `-32602` is the required answer.
    assert_eq!(
        findings_for(
            check,
            &incomplete_envelope(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"x"}}"#
            )
        ),
        Vec::<String>::new()
    );

    // A complete envelope answered with a result is the normal case.
    let complete = trace(&[
        client(
            0,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &complete), Vec::<String>::new());
}

#[test]
fn a_legacy_initialize_is_outside_the_rejection_rule() {
    // `basic/versioning`'s compatibility matrix makes the code implementation-
    // defined for exactly this exchange — `initialize` is both an unknown method
    // and a request without the `_meta` envelope — so a server answering -32601
    // has picked one of the two applicable rules and conforms. Every legacy →
    // modern capture is this exchange, so getting it wrong would put a MUST
    // failure on all of them.
    let check = "meta.missing-required-field-rejected";
    let handshake = trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &handshake), Vec::<String>::new());

    // The exemption is by method, not a blanket amnesty: any other method with
    // the same defect is still reported.
    let other = trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"protocolVersion":"2025-11-25"}}"#,
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &other).len(), 1);
}

#[test]
fn the_missing_capability_error_must_name_what_was_missing() {
    let check = "meta.missing-capability-error";

    // Absent, empty, and wrongly-typed each their own finding. The array case
    // is here because this check once *required* it: the schema types
    // `requiredCapabilities` as a `ClientCapabilities` object, so an array is
    // as malformed as a bare string.
    assert_eq!(findings_for(check, &capability_error("")).len(), 1);
    assert_eq!(
        findings_for(
            check,
            &capability_error(r#","data":{"requiredCapabilities":{}}"#)
        )
        .len(),
        1
    );
    assert_eq!(
        findings_for(
            check,
            &capability_error(r#","data":{"requiredCapabilities":"elicitation"}"#)
        )
        .len(),
        1
    );
    assert_eq!(
        findings_for(
            check,
            &capability_error(r#","data":{"requiredCapabilities":["elicitation"]}"#)
        )
        .len(),
        1,
        "an array is not a ClientCapabilities object"
    );

    // The shape the schema defines, and the shape rmcp's
    // `ErrorData::missing_required_client_capability` produces.
    assert_eq!(
        findings_for(
            check,
            &capability_error(r#","data":{"requiredCapabilities":{"elicitation":{}}}"#)
        ),
        Vec::<String>::new()
    );

    // Any other error code is outside this clause.
    let other = capability_error("").replace("-32021", "-32602");
    assert_eq!(findings_for(check, &other), Vec::<String>::new());
}

#[test]
fn subscription_tagging_binds_only_listen_streams_and_only_untagged_notifications() {
    let check = "meta.subscription-id-present";

    // Untagged change notification: the violation.
    assert_eq!(
        findings_for(
            check,
            &listening(r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#)
        )
        .len(),
        1
    );

    // Tagged with the listen request's id: silent.
    let tagged = |tag: &str| {
        listening(&format!(
            r#"{{"jsonrpc":"2.0","method":"notifications/tools/list_changed","params":{{"_meta":{{"io.modelcontextprotocol/subscriptionId":{tag}}}}}}}"#
        ))
    };
    assert_eq!(findings_for(check, &tagged("1")), Vec::<String>::new());

    // Tagged with anything else: the tag correlates the notification with no
    // subscription. `"1"` is not the id `1` — JSON-RPC ids are compared as
    // values — and `null` is not an id at all.
    for tag in [r#""s1""#, r#""1""#, "2", "null"] {
        let findings = findings_for(check, &tagged(tag));
        assert_eq!(findings.len(), 1, "{tag}: {findings:?}");
        assert!(findings[0].contains("not the id of any"), "{findings:?}");
    }

    // Request-scoped notifications ride their request's stream, not the listen
    // stream, so neither of these is tagged and neither is a finding.
    for method in ["notifications/progress", "notifications/message"] {
        assert!(
            findings_for(
                check,
                &listening(&format!(r#"{{"jsonrpc":"2.0","method":"{method}"}}"#))
            )
            .is_empty(),
            "{method} is request-scoped"
        );
    }

    // A server *request* on the stream carries both an id and a method; it is
    // TRAN-066's defect, not an untagged notification.
    assert_eq!(
        findings_for(
            check,
            &listening(r#"{"jsonrpc":"2.0","id":7,"method":"elicitation/create","params":{}}"#)
        ),
        Vec::<String>::new()
    );

    // A message with neither an id nor a method is malformed, and BASE-047's.
    assert_eq!(
        findings_for(check, &listening(r#"{"jsonrpc":"2.0"}"#)),
        Vec::<String>::new()
    );

    // A response is not a notification.
    assert_eq!(
        findings_for(
            check,
            &listening(r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#)
        ),
        Vec::<String>::new()
    );

    // Without a listen stream in the trace the clause does not bind at all.
    let unsubscribed = trace(&[
        client(
            0,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &unsubscribed), Vec::<String>::new());
}

#[test]
fn the_http_status_clauses_judge_only_their_own_error_code() {
    // Both share one helper, so the code each passes is the only thing that
    // distinguishes them. The `-32602` leg rides an *incomplete* envelope
    // because that is the only `-32602` BASE-032 binds; see the test below.
    const INCOMPLETE: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}"#;
    for (check, code, meta) in [
        (
            "meta.missing-required-field-http-status",
            -32602,
            INCOMPLETE,
        ),
        ("meta.missing-capability-http-status", -32021, META),
    ] {
        let document = trace(&[
            client(
                0,
                &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{meta}}}}}"#),
            ),
            status(1, 500),
            server(
                2,
                &format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":{code},"message":"x"}}}}"#),
            ),
        ]);
        assert_eq!(findings_for(check, &document).len(), 1, "{check}");
        assert!(
            findings_for(check, &document.replace("500", "400")).is_empty(),
            "{check} accepts 400"
        );
        // The sibling clause's code is not this one's business.
        let other = document.replace(&code.to_string(), "-32700");
        assert!(findings_for(check, &other).is_empty(), "{check}");
    }
}

#[test]
fn the_status_clause_binds_only_the_malformed_envelope_that_drew_the_error() {
    // `2026-07-28` *replaced* `-32002` with `-32602`, so a conforming server
    // now answers resource-not-found with the same code a malformed envelope
    // draws — and BASE-032 says nothing about that answer's HTTP status. Until
    // the enriched HTTP capture carried one, every `-32602` in every recording
    // was a malformed envelope and the difference could not show; judged
    // broadly, this check reported a conforming server for a clause that does
    // not bind it.
    let well_formed = trace(&[
        client(
            0,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{{{META},"uri":"test://gone"}}}}"#
            ),
        ),
        status(1, 404),
        server(
            2,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"resource not found"}}"#,
        ),
    ]);
    assert!(
        findings_for("meta.missing-required-field-http-status", &well_formed).is_empty(),
        "a -32602 that answers a well-formed request is not this clause's subject"
    );
    // The same status, against a request that *was* malformed, still fails.
    let malformed = well_formed.replace(
        r#""io.modelcontextprotocol/clientCapabilities":{}"#,
        r#""unrelated":{}"#,
    );
    assert_eq!(
        findings_for("meta.missing-required-field-http-status", &malformed).len(),
        1,
        "{malformed}"
    );
}

#[test]
fn the_server_teardown_of_a_subscription_needs_no_tag() {
    // The cancellation page mandates this exact message on stdio; it names the
    // subscription in `requestId`, which is what the tag would have said.
    let check = "meta.subscription-id-present";
    let teardown = r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1,"reason":"shutdown"}}"#;
    assert_eq!(
        findings_for(check, &listening(teardown)),
        Vec::<String>::new()
    );
    // A cancellation naming anything but a listen request is still untagged
    // traffic on a session with a listen stream.
    let other = teardown.replace(r#""requestId":1"#, r#""requestId":7"#);
    assert_eq!(findings_for(check, &listening(&other)).len(), 1);
}

#[test]
fn a_reused_id_is_judged_by_the_request_its_answer_pairs_with() {
    // BASE-045 allows reusing an id once answered; keyed by id alone, a
    // well-formed request's result was held against a malformed one sharing it.
    const INCOMPLETE: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}"#;
    let check = "meta.missing-required-field-rejected";
    let ask = |seq, meta: &str| {
        client(
            seq,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{{meta}}}}}"#),
        )
    };
    let ok = |seq| {
        server(
            seq,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","tools":[]}}"#,
        )
    };
    let rejected = server(
        1,
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"x"}}"#,
    );
    let malformed_first = trace(&[ask(0, INCOMPLETE), rejected, ask(2, META), ok(3)]);
    assert_eq!(findings_for(check, &malformed_first), Vec::<String>::new());
    let malformed_second = trace(&[ask(0, META), ok(1), ask(2, INCOMPLETE), ok(3)]);
    let findings = findings_for(check, &malformed_second);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("seq 2"), "{findings:?}");
}

#[test]
fn a_null_id_rejection_the_framing_ties_answers_the_malformed_request() {
    // On HTTP the status ties a null-id `-32602` to its POST. Paired by id
    // alone it answered nothing, the malformed request stayed open, and the
    // well-formed retry reusing its id had its result held against it.
    const INCOMPLETE: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}"#;
    let ask = |seq, meta: &str| {
        client(
            seq,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{{meta}}}}}"#),
        )
    };
    let session = |rejection_status| {
        trace(&[
            post(0, "{}"),
            ask(1, INCOMPLETE),
            status(2, rejection_status),
            server(
                3,
                r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32602,"message":"x"}}"#,
            ),
            post(4, "{}"),
            ask(5, META),
            status(6, 200),
            server(
                7,
                r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","tools":[]}}"#,
            ),
        ])
    };
    for check in [
        "meta.missing-required-field-rejected",
        "meta.missing-required-field-http-status",
    ] {
        let findings = findings_for(check, &session(400));
        assert!(findings.is_empty(), "{check}: {findings:?}");
    }
    // The null-id answer is the one BASE-032 judges, so its status counts.
    let findings = findings_for("meta.missing-required-field-http-status", &session(500));
    assert_eq!(findings.len(), 1, "{findings:?}");
}
