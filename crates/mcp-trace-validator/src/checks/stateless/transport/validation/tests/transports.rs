// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The unsupported-version rule on each transport: `VERS-001` binds every
//! transport, `TRAN-074` Streamable HTTP alone.

#![allow(clippy::expect_used)]

use super::*;
use crate::checks;
use crate::checks::stateless::testkit::events;
use crate::context::TraceContext;

/// The findings and the number of subjects `check` examined in `trace`.
fn judged(check: &str, trace: &str) -> (Vec<String>, u32) {
    let events = events(trace);
    let outcome = checks::find(check)
        .expect("check is registered")
        .run(&TraceContext::new(&events));
    let details = outcome.findings.into_iter().map(|f| f.detail).collect();
    (details, outcome.subjects)
}

/// The same session over stdio: no HTTP events, every message on stdio.
fn on_stdio(trace: &str) -> String {
    trace
        .lines()
        .filter(|line| !line.contains(r#""kind":"http""#))
        .map(|line| line.replace(r#""transport":"streamable-http""#, r#""transport":"stdio""#))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tran_074_judges_streamable_http_only_and_vers_001_every_transport() {
    let every = "transport.unsupported-version-error";
    let http = "transport.unsupported-version-error-http";
    // An unsupported version answered with a result, and a `-32022` with no
    // `data.supported`: one failure of each half.
    let answered = after_discovery(
        r#"["2026-07-28"]"#,
        "2025-11-25",
        r#"{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","tools":[]}}"#,
    );
    let unlisted = after_discovery(
        r#"["2026-07-28"]"#,
        "2025-11-25",
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32022,"message":"x"}}"#,
    );
    for session in [&answered, &unlisted] {
        let (over_http, examined) = judged(http, session);
        assert_eq!(over_http.len(), 1, "{over_http:?}");
        assert!(examined > 0);
        assert_eq!(judged(every, session).0, over_http);

        // Over stdio the same exchange is VERS-001's failure and no evidence
        // about TRAN-074 at all: not a pass, nothing examined.
        let stdio = on_stdio(session);
        assert_eq!(judged(every, &stdio).0, over_http);
        assert_eq!(judged(http, &stdio), (Vec::<String>::new(), 0));
    }
}
