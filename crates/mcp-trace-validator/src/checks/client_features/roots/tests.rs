// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `client/roots` checks.

use super::super::testkit::{
    ask, asked, call, client, findings_for, input_required, reply, reply_error, round, server,
    session, subjects_for,
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

/// A stdio lifecycle record.
fn lifecycle(seq: u64, event: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"{event}"}}"#
    )
}

/// The client's `roots/list` result listing `uris`.
fn listing(seq: u64, id: u64, uris: &[&str]) -> String {
    let roots: Vec<String> = uris
        .iter()
        .map(|uri| format!(r#"{{"uri":"{uri}","name":"n{seq}"}}"#))
        .collect();
    reply(seq, id, &format!(r#"{{"roots":[{}]}}"#, roots.join(",")))
}

const CHANGED: &str = "roots.list-changed-notified";
const LISTENING: &str = r#"{"roots":{"listChanged":true}}"#;
const NOTIFY: &str = r#"{"jsonrpc":"2.0","method":"notifications/roots/list_changed"}"#;

/// Two `roots/list` exchanges at seq 3–4 and 6–7 listing `first` and `second`,
/// with `between` at seq 5 and `after` from seq 8.
fn two_listings(
    capabilities: &str,
    first: &[&str],
    between: &[String],
    second: &[&str],
    after: &[String],
) -> String {
    let mut rest = vec![ask(3, 1, "roots/list", "{}"), listing(4, 1, first)];
    rest.extend_from_slice(between);
    rest.push(ask(6, 2, "roots/list", "{}"));
    rest.push(listing(7, 2, second));
    rest.extend_from_slice(after);
    session(capabilities, &rest)
}

/// Two listings that differ, with nothing between them.
fn differing(after: &[String]) -> String {
    two_listings(LISTENING, &["file:///a"], &[], &["file:///b"], after)
}

#[test]
fn roots_that_changed_without_a_notification_are_reported() {
    let close = [lifecycle(8, "transport-close")];
    let silent = two_listings(
        LISTENING,
        &["file:///a"],
        &[],
        &["file:///a", "file:///b"],
        &close,
    );
    let found = findings_for(CHANGED, &silent);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("than at seq 4") && found[0].contains("notifications/roots/list_changed"),
        "{found:?}"
    );
    assert_eq!(subjects_for(CHANGED, &silent), 1);
    // A client cut off is not judged: its notification may have been next.
    let aborted = differing(&[lifecycle(8, "transport-abort")]);
    assert_eq!(findings_for(CHANGED, &aborted).len(), 0);
    assert_eq!(subjects_for(CHANGED, &aborted), 0);
    // A root removed is a change too.
    let removed = two_listings(
        LISTENING,
        &["file:///a", "file:///b"],
        &[],
        &["file:///a"],
        &close,
    );
    assert_eq!(findings_for(CHANGED, &removed).len(), 1);
}

#[test]
fn unchanged_roots_and_notified_changes_pass() {
    let close = [lifecycle(9, "transport-close")];
    // The same set, in another order and under other names.
    let same = two_listings(
        LISTENING,
        &["file:///a", "file:///b"],
        &[],
        &["file:///b", "file:///a"],
        &close,
    );
    assert_eq!(findings_for(CHANGED, &same), Vec::<String>::new());
    assert_eq!(subjects_for(CHANGED, &same), 1);
    // Notified between the results, after the second, or before the first.
    let between = two_listings(
        LISTENING,
        &["file:///a"],
        &[client(5, NOTIFY)],
        &["file:///b"],
        &close,
    );
    let after = differing(&[client(8, NOTIFY), lifecycle(9, "transport-close")]);
    for trace in [between, after] {
        assert_eq!(findings_for(CHANGED, &trace), Vec::<String>::new());
        assert_eq!(subjects_for(CHANGED, &trace), 1);
    }
    let early = [
        client(3, NOTIFY),
        ask(4, 1, "roots/list", "{}"),
        listing(5, 1, &["file:///a"]),
        ask(6, 2, "roots/list", "{}"),
        listing(7, 2, &["file:///b"]),
        lifecycle(8, "transport-close"),
    ];
    assert_eq!(
        findings_for(CHANGED, &session(LISTENING, &early)),
        Vec::<String>::new()
    );
    // The server's notification of that name is not the client's; nor is a
    // client request of that name, or another notification.
    let request = r#"{"jsonrpc":"2.0","id":9,"method":"notifications/roots/list_changed"}"#;
    let other = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    for stray in [server(5, NOTIFY), client(5, request), client(5, other)] {
        let trace = two_listings(
            LISTENING,
            &["file:///a"],
            std::slice::from_ref(&stray),
            &["file:///b"],
            &close,
        );
        assert_eq!(findings_for(CHANGED, &trace).len(), 1, "{stray}");
    }
}

#[test]
fn a_session_not_shown_to_have_ended_is_not_judged() {
    // No lifecycle event: the notification may be about to come.
    assert_eq!(subjects_for(CHANGED, &differing(&[])), 0);
    assert_eq!(findings_for(CHANGED, &differing(&[])), Vec::<String>::new());
    // The transport reopened before it closed: not one session.
    let reopened = differing(&[
        lifecycle(8, "transport-open"),
        lifecycle(9, "transport-close"),
    ]);
    assert_eq!(subjects_for(CHANGED, &reopened), 0);
    // The close fell between the two results: the second is another process's.
    let split = two_listings(
        LISTENING,
        &["file:///a"],
        &[lifecycle(5, "transport-close")],
        &["file:///b"],
        &[lifecycle(8, "transport-close")],
    );
    assert_eq!(subjects_for(CHANGED, &split), 0);
    assert_eq!(findings_for(CHANGED, &split), Vec::<String>::new());
    // Streamable HTTP records nothing that ends a session.
    let http = differing(&[lifecycle(8, "transport-close")]).replace(
        r#""transport":"stdio","kind":"lifecycle""#,
        r#""transport":"streamable-http","kind":"lifecycle""#,
    );
    assert_eq!(subjects_for(CHANGED, &http), 0);
    // No `initialize` in the recording: nothing says what the client declared
    // or where its session began.
    let headless = super::super::testkit::trace(&[
        ask(3, 1, "roots/list", "{}"),
        listing(4, 1, &["file:///a"]),
        ask(6, 2, "roots/list", "{}"),
        listing(7, 2, &["file:///b"]),
        lifecycle(8, "transport-close"),
    ]);
    assert_eq!(subjects_for(CHANGED, &headless), 0);
}

#[test]
fn the_transport_opening_before_initialize_is_not_a_second_session() {
    let shifted = |line: &str| {
        let rest = line.strip_prefix(r#"{"seq":"#).unwrap_or(line);
        let (seq, tail) = rest.split_once(',').unwrap_or(("0", rest));
        format!(r#"{{"seq":{},{tail}"#, seq.parse::<u64>().unwrap_or(0) + 1)
    };
    let body: Vec<String> = differing(&[lifecycle(8, "transport-close")])
        .lines()
        .map(shifted)
        .collect();
    // The open at seq 0 precedes `initialize` at seq 1.
    let opened = format!("{}\n{}", lifecycle(0, "transport-open"), body.join("\n"));
    assert_eq!(findings_for(CHANGED, &opened).len(), 1);
    assert_eq!(subjects_for(CHANGED, &opened), 1);
}

#[test]
fn only_a_client_that_declared_list_changed_is_bound() {
    let close = [lifecycle(8, "transport-close")];
    for capabilities in [
        r#"{"roots":{}}"#,
        r#"{"roots":{"listChanged":false}}"#,
        "{}",
    ] {
        let trace = two_listings(capabilities, &["file:///a"], &[], &["file:///b"], &close);
        assert_eq!(subjects_for(CHANGED, &trace), 0, "{capabilities}");
        assert_eq!(findings_for(CHANGED, &trace), Vec::<String>::new());
    }
}

#[test]
fn only_readable_listings_are_compared() {
    // An error and unreadable results between two listings are passed over;
    // the listings on either side still differ.
    let trace = session(
        LISTENING,
        &[
            ask(3, 1, "roots/list", "{}"),
            listing(4, 1, &["file:///a"]),
            ask(5, 2, "roots/list", "{}"),
            reply_error(6, 2, -32603),
            ask(7, 3, "roots/list", "{}"),
            reply(8, 3, r#"{"roots":[{"name":"no uri"}]}"#),
            ask(9, 4, "roots/list", "{}"),
            reply(10, 4, r#"{"roots":"none"}"#),
            ask(11, 5, "roots/list", "{}"),
            reply(12, 5, "{}"),
            ask(13, 6, "roots/list", "{}"),
            listing(14, 6, &["file:///b"]),
            lifecycle(15, "transport-close"),
        ],
    );
    assert_eq!(subjects_for(CHANGED, &trace), 1);
    let found = findings_for(CHANGED, &trace);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("than at seq 4"), "{found:?}");
    // One listing is no pair; three are two.
    let one = session(
        LISTENING,
        &[
            ask(3, 1, "roots/list", "{}"),
            listing(4, 1, &["file:///a"]),
            lifecycle(5, "transport-close"),
        ],
    );
    assert_eq!(subjects_for(CHANGED, &one), 0);
    let three = session(
        LISTENING,
        &[
            ask(3, 1, "roots/list", "{}"),
            listing(4, 1, &["file:///a"]),
            ask(5, 2, "roots/list", "{}"),
            listing(6, 2, &["file:///a"]),
            ask(7, 3, "roots/list", "{}"),
            listing(8, 3, &[]),
            lifecycle(9, "transport-close"),
        ],
    );
    assert_eq!(subjects_for(CHANGED, &three), 2);
    assert_eq!(findings_for(CHANGED, &three).len(), 1);
    // Another method's answers are not listings.
    let pings = session(
        LISTENING,
        &[
            ask(3, 1, "ping", "{}"),
            listing(4, 1, &["file:///a"]),
            ask(5, 2, "ping", "{}"),
            listing(6, 2, &["file:///b"]),
            lifecycle(7, "transport-close"),
        ],
    );
    assert_eq!(subjects_for(CHANGED, &pings), 0);
}

#[test]
fn roots_answered_inside_a_retry_are_not_this_clauses() {
    // `2026-07-28` has no roots notification; a trace mixing a handshake with
    // MRTR rounds answers roots twice, differently, and is not judged.
    let round_at = |id: u64, uri: &str| {
        [
            call(3 * id, id, LISTENING, ""),
            input_required(3 * id + 1, id, ENTRY),
            call(
                3 * id + 2,
                id + 10,
                LISTENING,
                &format!(
                    r#""inputResponses":{{"a":{{"roots":[{{"uri":"{uri}"}}]}}}},"requestState":"st","#
                ),
            ),
        ]
    };
    let mut rest = round_at(1, "file:///a").to_vec();
    rest.extend(round_at(2, "file:///b"));
    rest.push(lifecycle(9, "transport-close"));
    assert_eq!(subjects_for(CHANGED, &session(LISTENING, &rest)), 0);
}
