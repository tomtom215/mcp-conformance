// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use super::*;
use crate::judging::{Class, Judged, UnjudgedSession};

/// No failures, as a typed value to compare a list against.
fn no_failures() -> Vec<mcp_conformance_cli::agreement::ValidatorFailure> {
    Vec::new()
}

/// A clean handshake.
const HANDSHAKE: &str = concat!(
    r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#,
    "\n",
    r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#,
    "\n",
    r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
    "\n"
);

/// A `ping` the server answers with a result that is not empty: PING-001 and
/// nothing else, held against the server that sent it.
pub(crate) const SERVER_BREAKS_PING_001: &str = concat!(
    r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"ping"}}"#,
    "\n",
    r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"extra":1}}}"#,
    "\n"
);

/// A `ping` that reuses the `initialize` request's id: BASE-003, held against
/// the client that sent it.
pub(crate) const CLIENT_BREAKS_BASE_003: &str = concat!(
    r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}"#,
    "\n",
    r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#,
    "\n"
);

/// A session judged and viewed for `party`.
pub(crate) fn session(party: Party, body: &str) -> Judged {
    let judgment = mcp_trace_validator::judge::judge(&format!("{HANDSHAKE}{body}")).unwrap();
    Judged {
        name: "001.jsonl".to_owned(),
        path: PathBuf::from("/w/traces/001.jsonl"),
        events: judgment.events,
        reports: judgment
            .reports
            .into_iter()
            .map(|mut report| {
                report.party_view = Some(party);
                report
            })
            .collect(),
    }
}

/// A judged session of a server under test: a clean handshake, plus (when it
/// should fail) a `ping` answered with a non-empty result, which breaks
/// PING-001, the server's own.
pub(crate) fn judged(fail: bool) -> Judged {
    session(
        Party::Server,
        if fail { SERVER_BREAKS_PING_001 } else { "" },
    )
}

/// A judged session of a server under test whose *suite's client* breaks
/// BASE-003: the finding is the client's.
pub(crate) fn judged_client_fault() -> Judged {
    session(Party::Server, CLIENT_BREAKS_BASE_003)
}

pub(crate) fn outcome(suite_passed: bool, judged: Vec<Judged>, baseline: &Baseline) -> Outcome {
    outcome_for(Party::Server, suite_passed, judged, baseline)
}

pub(crate) fn outcome_for(
    party: Party,
    suite_passed: bool,
    judged: Vec<Judged>,
    baseline: &Baseline,
) -> Outcome {
    let judging = Judging {
        judged,
        // The recording of a request the server refused: nothing to judge.
        unjudged: vec![UnjudgedSession {
            record: mcp_trace_validator::report::Unjudged::new(
                "/w/traces/002.jsonl",
                "the server sent no message",
            ),
            name: "002.jsonl".to_owned(),
            class: Class::Informational,
        }],
        stale_expected: vec![],
    };
    let reconciliation = agreement::reconcile_for(baseline, &judging.failures(), party);
    Outcome {
        subject: "http://localhost:3000/mcp".to_owned(),
        spec_version: "2025-11-25".to_owned(),
        party,
        suite_passed,
        scenarios: vec![],
        not_run: vec![("dns-rebinding-protection".to_owned(), "why")],
        judging,
        reconciliation,
        fixtures: None,
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
        r#"{"policy":"p","divergences":[{"requirement":"PING-001","class":"our-bug","upstream":"https://e.example/1"}]}"#,
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

/// The audit's A08: a session the run recorded and could not judge was a
/// `pass` with exit 0. One that holds nothing to judge (the server never
/// answered) still is; one that is an error, or an expectation of one that did
/// not occur, is a run that could not be judged, whatever the suite said.
#[test]
fn a_session_the_run_could_not_judge_makes_it_incomplete_not_a_pass() {
    let none = Baseline::default();
    let mut run = outcome(true, vec![judged(false)], &none);
    assert_eq!(run.exit_code(), EXIT_OK, "informational: nothing was lost");
    assert!(!run.judging.has_errors());

    run.judging.unjudged[0].class = Class::Error;
    assert!(run.judging.has_errors());
    assert_eq!(run.exit_code(), EXIT_USAGE);

    let mut failing = outcome(false, vec![judged(true)], &none);
    failing.judging.unjudged[0].class = Class::Error;
    assert_eq!(
        failing.exit_code(),
        EXIT_USAGE,
        "an incomplete judgment is not reported as findings"
    );

    let mut stale = outcome(true, vec![judged(false)], &none);
    stale.judging.stale_expected = vec!["sse-retry".to_owned()];
    assert!(stale.judging.has_errors());
    assert_eq!(
        stale.exit_code(),
        EXIT_USAGE,
        "an expected session that did not occur"
    );
}

/// The audit's finding 1.1: the suite's own client breaks a clause (here
/// BASE-003, a reused request id) in a session with a conforming server. The
/// finding is the client's; a server under test is not held to it, and a client
/// under test, whose finding it is, is.
#[test]
fn the_other_ends_failure_is_set_aside_and_the_subjects_own_breaks_agreement() {
    let none = Baseline::default();
    let server = outcome(true, vec![judged_client_fault()], &none);
    let set_aside = &server.reconciliation.set_aside;
    assert_eq!(set_aside.len(), 1, "{:?}", server.reconciliation);
    assert_eq!(set_aside[0].requirement, "BASE-003");
    assert_eq!(set_aside[0].party, Some(Party::Client));
    assert_eq!(server.reconciliation.unexplained, no_failures());
    assert_eq!(server.exit_code(), EXIT_OK, "not the server's to fix");
    assert_eq!(
        server.judging.verdict(),
        Verdict::Pass,
        "the session reads clean for the server"
    );

    let client = outcome_for(
        Party::Client,
        true,
        vec![session(Party::Client, CLIENT_BREAKS_BASE_003)],
        &none,
    );
    assert_eq!(client.reconciliation.unexplained.len(), 1);
    assert_eq!(client.reconciliation.set_aside, no_failures());
    assert_eq!(client.exit_code(), EXIT_FINDINGS, "the client's own");
    assert_eq!(client.judging.verdict(), Verdict::Fail);

    // And the server's own failure is the server's, whichever way it is read.
    let own = outcome(true, vec![judged(true)], &none);
    assert_eq!(own.reconciliation.unexplained[0].party, Some(Party::Server));
    assert_eq!(own.exit_code(), EXIT_FINDINGS);
    let suites = outcome_for(
        Party::Client,
        true,
        vec![session(Party::Client, SERVER_BREAKS_PING_001)],
        &none,
    );
    assert_eq!(suites.reconciliation.set_aside[0].requirement, "PING-001");
    assert_eq!(suites.exit_code(), EXIT_OK, "the suite's own server's");
}

/// The baseline still explains a failure held against either end, and an entry
/// that explains only the suite's own end's failure is live, not stale.
#[test]
fn a_baseline_entry_for_the_suites_own_failure_stays_live() {
    let explains = Baseline::parse(
        r#"{"policy":"p","divergences":[{"requirement":"BASE-003","class":"suite-bug","upstream":"https://e.example/1"}]}"#,
    )
    .unwrap();
    let run = outcome(true, vec![judged_client_fault()], &explains);
    assert_eq!(run.reconciliation.explained.len(), 1);
    assert_eq!(run.reconciliation.set_aside, no_failures());
    assert!(
        run.reconciliation.stale.is_empty(),
        "{:?}",
        run.reconciliation
    );
    assert_eq!(run.exit_code(), EXIT_OK);
}

#[test]
fn the_subject_is_the_end_of_the_session_its_flag_names() {
    let subject = |url: bool, command: bool, client: bool| Subject {
        url: url.then(|| "http://x/mcp".to_owned()),
        command: command.then(|| "server".to_owned()),
        client_command: client.then(|| "client".to_owned()),
    };
    assert_eq!(subject_party(&subject(true, false, false)), Party::Server);
    assert_eq!(subject_party(&subject(false, true, false)), Party::Server);
    assert_eq!(subject_party(&subject(false, false, true)), Party::Client);
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
fn only_a_server_elsewhere_gets_the_host_note() {
    assert_eq!(host_note(true), None);
    let note = host_note(false).unwrap();
    assert!(
        note.starts_with("the recording proxy sends the server's own name as `Host`")
            && note.contains("dns-rebinding-protection"),
        "{note}"
    );
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
