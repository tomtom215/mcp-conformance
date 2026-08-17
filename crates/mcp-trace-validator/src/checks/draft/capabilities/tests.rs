// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `2026-07-28` capability-declaration checks.
//!
//! The case these exist for is the one the `2025-11-25` checks get wrong here: a
//! session with no `server/discover` in it. Those checks abstain on *every*
//! `2026-07-28` trace, because they look for an `initialize` result. These
//! abstain only when there is genuinely no declaration to read, and the tests
//! pin both halves — silence is not a denial, but a discover result that omits a
//! capability is.

use crate::checks::draft::testkit::{client, findings_for, server, trace};

const COMPLETIONS: &str = "capabilities.completions-declared";
const LOGGING: &str = "capabilities.logging-declared";

/// A `server/discover` exchange declaring `capabilities`.
fn discovered(capabilities: &str) -> Vec<String> {
    vec![
        client(
            0,
            r#"{"jsonrpc":"2.0","id":90,"method":"server/discover","params":{"_meta":{}}}"#,
        ),
        server(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":90,"result":{{"resultType":"complete","ttlMs":0,"capabilities":{capabilities}}}}}"#
            ),
        ),
    ]
}

/// A client request for `method`, answered with a bare `complete` result.
fn served(seq: u64, method: &str) -> Vec<String> {
    vec![
        client(
            seq,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{seq},"method":"{method}","params":{{"_meta":{{}}}}}}"#
            ),
        ),
        server(
            seq + 1,
            &format!(r#"{{"jsonrpc":"2.0","id":{seq},"result":{{"resultType":"complete"}}}}"#),
        ),
    ]
}

/// A server log notification.
fn logged(seq: u64) -> String {
    server(
        seq,
        r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"x"}}"#,
    )
}

#[test]
fn a_session_that_never_probed_is_not_judged() {
    // This is the whole reason these checks exist. The `2025-11-25` siblings
    // abstain here *and* on the declared case, because they read the handshake;
    // these abstain only when there is no declaration surface at all.
    let session = trace(&served(0, "tools/call"));
    for check in [COMPLETIONS, LOGGING] {
        assert!(
            findings_for(check, &session).is_empty(),
            "{check} judged a session with no discovery"
        );
    }
}

#[test]
fn logging_is_judged_by_the_notification_it_emits() {
    let mut undeclared = discovered("{}");
    undeclared.push(logged(2));
    assert_eq!(findings_for(LOGGING, &trace(&undeclared)).len(), 1);

    let mut declared = discovered(r#"{"logging":{}}"#);
    declared.push(logged(2));
    assert!(findings_for(LOGGING, &trace(&declared)).is_empty());
}
