// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `client/roots` checks.

use super::super::testkit::{
    ask, asked, findings_for, reply, reply_error, round, session, subjects_for,
};

const ENTRY: &str = r#""a":{"method":"roots/list","params":{}}"#;

#[test]
fn every_root_is_a_file_uri() {
    const CHECK: &str = "roots.uri-file-scheme";
    let caps = r#"{"roots":{}}"#;
    for (roots, subjects, expected) in [
        (r#"[{"uri":"file:///home/user/project","name":"p"}]"#, 1, 0),
        (
            r#"[{"uri":"FILE:///C:/x"},{"uri":"file://host/share"}]"#,
            2,
            0,
        ),
        (r#"[{"uri":"https://example.com/repo"}]"#, 1, 1),
        (r#"[{"uri":"file:/no-authority"}]"#, 1, 1),
        (r#"[{"uri":"file:"}]"#, 1, 1),
        (r#"[{"name":"no uri"},{"uri":7}]"#, 2, 2),
        ("[]", 0, 0),
    ] {
        let result = format!(r#"{{"roots":{roots}}}"#);
        let mrtr = round(caps, ENTRY, &format!(r#""a":{result}"#), caps);
        let direct = session(caps, &[ask(3, 1, "roots/list", "{}"), reply(4, 1, &result)]);
        for trace in [mrtr, direct] {
            assert_eq!(findings_for(CHECK, &trace).len(), expected, "{roots}");
            assert_eq!(subjects_for(CHECK, &trace), subjects, "{roots}");
        }
    }
    let found = findings_for(
        CHECK,
        &round(
            caps,
            ENTRY,
            r#""a":{"roots":[{"uri":"https://x"},{}]}"#,
            caps,
        ),
    );
    assert!(
        found[0].contains("`roots[0]`") && found[0].contains(r#""https://x""#),
        "{found:?}"
    );
    assert!(
        found[1].contains("`roots[1]`") && found[1].contains("no string `uri`"),
        "{found:?}"
    );
    let refused = session(
        caps,
        &[ask(3, 1, "roots/list", "{}"), reply_error(4, 1, -32603)],
    );
    assert_eq!(subjects_for(CHECK, &refused), 0);
}

#[test]
fn the_server_checks_for_roots_before_asking() {
    const CHECK: &str = "roots.capability-checked";
    for (capabilities, expected) in [(r#"{"roots":{}}"#, 0), (r#"{"sampling":{}}"#, 1), ("{}", 1)] {
        let mrtr = asked(capabilities, ENTRY);
        let direct = session(capabilities, &[ask(3, 1, "roots/list", "{}")]);
        for trace in [mrtr, direct] {
            assert_eq!(
                findings_for(CHECK, &trace).len(),
                expected,
                "{capabilities}"
            );
            assert_eq!(subjects_for(CHECK, &trace), 1);
        }
    }
    let found = findings_for(CHECK, &asked("{}", ENTRY));
    assert!(
        found[0].contains("`roots`") && found[0].contains("clientCapabilities"),
        "{found:?}"
    );
    let headless = super::super::testkit::trace(&[ask(0, 1, "roots/list", "{}")]);
    assert_eq!(subjects_for(CHECK, &headless), 0);
}

#[test]
fn a_client_without_roots_answers_method_not_found() {
    const CHECK: &str = "roots.unsupported-rejected";
    let answered =
        |caps: &str, answer: String| session(caps, &[ask(3, 1, "roots/list", "{}"), answer]);
    assert_eq!(
        findings_for(CHECK, &answered("{}", reply_error(4, 1, -32601))),
        Vec::<String>::new()
    );
    assert_eq!(
        subjects_for(CHECK, &answered("{}", reply_error(4, 1, -32601))),
        1
    );
    let found = findings_for(CHECK, &answered("{}", reply_error(4, 1, -32603)));
    assert!(found[0].contains("error -32603"), "{found:?}");
    let found = findings_for(CHECK, &answered("{}", reply(4, 1, r#"{"roots":[]}"#)));
    assert!(found[0].contains("a result"), "{found:?}");
    // A client that declared roots, or an MRTR answer: not this clause's.
    assert_eq!(
        subjects_for(
            CHECK,
            &answered(r#"{"roots":{}}"#, reply_error(4, 1, -32603))
        ),
        0
    );
    assert_eq!(
        subjects_for(CHECK, &round("{}", ENTRY, r#""a":{"roots":[]}"#, "{}")),
        0
    );
}
