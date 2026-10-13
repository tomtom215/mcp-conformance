// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use super::*;
use crate::judging::Judged;

/// A judged session: a clean handshake, plus a reused id when it should fail.
pub(crate) fn judged(fail: bool) -> Judged {
    let mut trace = String::from(concat!(
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#,
        "\n",
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#,
        "\n",
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
        "\n"
    ));
    if fail {
        trace.push_str(concat!(
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}"#,
            "\n",
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#,
            "\n"
        ));
    }
    let judgment = mcp_trace_validator::judge::judge(&trace).unwrap();
    Judged {
        name: "001.jsonl".to_owned(),
        path: PathBuf::from("/w/traces/001.jsonl"),
        events: judgment.events,
        reports: judgment.reports,
    }
}

pub(crate) fn outcome(suite_passed: bool, judged: Vec<Judged>, baseline: &Baseline) -> Outcome {
    let judging = Judging {
        judged,
        unjudged: vec![mcp_trace_validator::report::Unjudged::new(
            "/w/traces/002.jsonl",
            "the server sent no message",
        )],
    };
    let reconciliation = agreement::reconcile(baseline, &judging.failures());
    Outcome {
        subject: "http://localhost:3000/mcp".to_owned(),
        spec_version: "2025-11-25".to_owned(),
        suite_passed,
        scenarios: vec![],
        not_run: vec![("dns-rebinding-protection".to_owned(), "why")],
        judging,
        reconciliation,
        baseline: None,
        work_dir: PathBuf::from("/w"),
    }
}

#[test]
fn the_exit_code_is_the_suite_and_the_agreement() {
    let none = Baseline::default();
    assert_eq!(
        outcome(true, vec![judged(false)], &none).exit_code(),
        EXIT_OK
    );
    assert_eq!(
        outcome(false, vec![judged(false)], &none).exit_code(),
        EXIT_FINDINGS
    );
    assert_eq!(
        outcome(true, vec![judged(true)], &none).exit_code(),
        EXIT_FINDINGS,
        "an unexplained failing clause is a disagreement"
    );
    let explains = Baseline::parse(
        r#"{"policy":"p","divergences":[{"requirement":"BASE-003","class":"our-bug","upstream":"https://e.example/1"}]}"#,
    )
    .unwrap();
    let explained = outcome(true, vec![judged(true)], &explains);
    assert_eq!(
        explained.reconciliation.explained.len(),
        1,
        "{:?}",
        explained.reconciliation
    );
    assert_eq!(explained.exit_code(), EXIT_OK, "an explained one is not");
    assert_eq!(
        outcome(true, vec![judged(false)], &explains).exit_code(),
        EXIT_FINDINGS,
        "a stale entry is a disagreement"
    );
    let mut unsupported = outcome(true, vec![judged(false)], &none);
    unsupported.judging.judged[0].reports[0].requirements[0].outcome =
        mcp_trace_validator::report::Outcome::Unsupported;
    unsupported.judging.judged[0].reports[0].totals.unsupported = 1;
    assert_eq!(
        unsupported.exit_code(),
        EXIT_USAGE,
        "a build that cannot judge"
    );
}

#[test]
fn a_stdio_server_is_run_against_the_default_set_less_the_http_transport() {
    let listed = [
        "ping",
        "dns-rebinding-protection",
        "server-sse-polling",
        "json-schema-2020-12",
        "server-sse-multiple-streams",
        "tools-list",
    ]
    .map(ToOwned::to_owned)
    .to_vec();
    let (run, not_run) = stdio_scenarios(listed);
    assert_eq!(run, ["ping", "tools-list"]);
    let names: Vec<&str> = not_run.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "dns-rebinding-protection",
            "server-sse-polling",
            "json-schema-2020-12",
            "server-sse-multiple-streams"
        ]
    );
    assert!(not_run[0].1.contains("streamable-HTTP transport"));
    assert!(
        not_run[1].1.contains("streamable-HTTP transport"),
        "transport wins over pending"
    );
    assert!(not_run[2].1.contains("pending"));
}

#[test]
fn a_url_is_shown_without_its_query() {
    let (upstream, path) = proxy::split("http://localhost:3000/mcp?key=s3cret").unwrap();
    assert_eq!(shown(&upstream, &path), "http://localhost:3000/mcp?…");
    let (upstream, path) = proxy::split("https://example.com/").unwrap();
    assert_eq!(shown(&upstream, &path), "https://example.com/");
}

#[test]
fn the_work_dir_is_new_or_empty_and_absolute() {
    let dir = std::env::temp_dir().join(format!("mcp-conformance-work-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let made = work_dir(Some(&dir)).unwrap();
    assert!(made.is_absolute() && made.is_dir());
    assert_eq!(work_dir(Some(&dir)).unwrap(), made, "an empty one is fine");
    std::fs::write(dir.join("x"), "").unwrap();
    assert!(work_dir(Some(&dir)).unwrap_err().contains("is not empty"));
    std::fs::remove_dir_all(&dir).unwrap();
    let fresh = work_dir(None).unwrap();
    assert!(fresh.starts_with(std::env::temp_dir()) || fresh.is_absolute());
    assert_ne!(work_dir(None).unwrap(), fresh, "a new one each run");
    std::fs::remove_dir_all(&fresh).ok();
}

#[test]
fn a_stop_is_not_a_verdict() {
    assert_eq!(finished(Ok(Ok(true))), Ok(true));
    assert_eq!(finished(Ok(Ok(false))), Ok(false));
    let stopped = finished(Ok(Err(mcp_trace_capture::Stop::Interrupt))).unwrap_err();
    assert!(stopped.starts_with("SIGINT: stopped"), "{stopped}");
    assert_eq!(finished(Err("npx".to_owned())), Err("npx".to_owned()));
}
