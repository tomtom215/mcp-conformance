// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Rejections answered with a null `id`: the HTTP framing ties each to its
//! POST, so it is judged as that POST's answer and nothing else's.

use super::*;

#[test]
fn a_null_id_refusal_answers_its_own_post_not_the_retry() {
    // A mismatched `Mcp-Name` refused with a null-id `-32020`, then a corrected
    // retry reusing the id and served. Paired by id alone, the refusal answered
    // nothing and the retry's result was held against the mismatched POST.
    let call = |seq, name: &str| {
        [
            post(
                seq,
                &format!(
                    r#"{{"mcp-protocol-version":"2026-07-28","mcp-method":"tools/call","mcp-name":"{name}"}}"#
                ),
            ),
            client(
                seq + 1,
                &format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"echo",{META}}}}}"#
                ),
            ),
        ]
    };
    let [wrong_post, wrong_body] = call(0, "wrong");
    let [right_post, right_body] = call(4, "echo");
    let document = trace(&[
        wrong_post,
        wrong_body,
        status(2, 400),
        error(3, "null", -32020),
        right_post,
        right_body,
        status(6, 200),
        server(7, RESULT),
    ]);
    let findings = findings_for("transport.header-body-match-validated", &document);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_null_id_answer_is_judged_once_as_its_posts_answer() {
    // The Python SDK's refusal of a version mismatch drops the id. The answer
    // is still that POST's, and it is reported once — not once as an exchange
    // and again as a framing-tied answer.
    let null_invalid = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"x"}}"#;
    let findings = findings_for(
        "transport.version-mismatch-rejected",
        &exchange("2025-11-25", "2026-07-28", 400, null_invalid),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("error -32600"), "{findings:?}");
}
