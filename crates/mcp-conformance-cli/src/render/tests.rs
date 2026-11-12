// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use mcp_conformance_cli::agreement::Baseline;
use mcp_conformance_cli::suite::Check;

use super::*;
use crate::fixtures::{Fixtures, Unlisted};
use crate::run::tests::{
    CLIENT_BREAKS_BASE_003, SERVER_BREAKS_PING_001, judged, judged_client_fault, outcome,
    outcome_for, session,
};

fn scenario(name: &str, statuses: &[(&str, Option<&str>)]) -> Scenario {
    let checks: Vec<Check> = statuses
        .iter()
        .map(|(status, message)| {
            serde_json::from_value(
                json!({"id": format!("{name}-check"), "status": status, "errorMessage": message}),
            )
            .unwrap()
        })
        .collect();
    scenario_of(name, &checks)
}

fn scenario_of(name: &str, checks: &[Check]) -> Scenario {
    // Scenarios are read from disk in the product; build one the same way.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mcp-conformance-render-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let inner = dir.join(format!("{name}-2026-10-13T00-00-00-000Z"));
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(
        inner.join("checks.json"),
        serde_json::to_string(checks).unwrap(),
    )
    .unwrap();
    let mut read = suite::read_results(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    read.remove(0)
}

fn run_outcome(fail: bool) -> Outcome {
    let mut outcome = outcome(true, vec![judged(fail)], &Baseline::default());
    outcome.scenarios = vec![
        scenario("ping", &[("SUCCESS", None)]),
        scenario(
            "tools-list",
            &[("SUCCESS", None), ("WARNING", Some("slow")), ("INFO", None)],
        ),
    ];
    outcome
}

#[test]
fn the_human_report_has_all_three_parts() {
    let text = human(&run_outcome(true), false);
    for needle in [
        "mcp-conformance test: http://localhost:3000/mcp",
        &format!(
            "official suite {}@{}, spec 2025-11-25",
            suite::PACKAGE,
            suite::VERSION
        ),
        "== The official suite ==",
        "  pass  ping",
        "  FAIL  tools-list",
        "          WARNING tools-list-check: slow",
        "  --    dns-rebinding-protection",
        "not run: why",
        "2 scenarios, 4 checks: 1 INFO, 2 SUCCESS, 1 WARNING; suite verdict: pass",
        "== The clause report: 2 recorded session(s) ==",
        "Viewed for the server: a finding tagged [client] is held against the suite's own \
         client and does not count toward a verdict; one tagged [unattributed] is counted.",
        "==> /w/traces/001.jsonl <==",
        "PING-001",
        "        [server] seq 4: ",
        "verdict (server findings only): fail\n",
        "==> /w/traces/002.jsonl <==\nnot judged (informational): the server sent no message",
        "2 sessions: 0 pass, 0 pass-with-warnings, 1 fail, 1 not judged; clause verdict (server \
         findings only): fail",
        "== Agreement ==",
        "subject: the server; the suite's own client is the other end.",
        "divergence baseline: none",
        "failing clauses: 1 (0 explained, 1 unexplained, 0 set aside); stale baseline entries: 0",
        "agreement: does not hold",
        "1 unexplained validator-vs-runner divergence(s):\n  PING-001 on 001.jsonl [server]: ",
        "record explained ones in a baseline (--divergences)",
        "\nverdict: fail\n",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    let clean = human(&run_outcome(false), true);
    assert!(clean.contains("agreement: holds"), "{clean}");
    assert!(clean.contains("\nverdict: pass\n"), "{clean}");
    assert!(
        clean.contains("PASS "),
        "--all lists passing clauses: {clean}"
    );
}

/// What is held against the suite's own end is in the report, labelled as
/// that end's, and moves neither a verdict nor the agreement.
#[test]
fn the_suites_own_ends_findings_are_shown_labelled_and_not_counted() {
    let mut server = outcome(true, vec![judged_client_fault()], &Baseline::default());
    server.scenarios = vec![scenario("ping", &[("SUCCESS", None)])];
    let text = human(&server, false);
    for needle in [
        "        [client] seq 3: ",
        "verdict (server findings only): pass \u{2014} 1 client finding set aside\n",
        "; clause verdict (server findings only): pass \u{2014} 1 client finding set aside\n",
        "failing clauses: 1 (0 explained, 0 unexplained, 1 set aside); stale baseline entries: 0",
        "agreement: holds\n",
        "failing clauses held against the suite's own client (shown, and not counted):\n  \
         BASE-003 on 001.jsonl [client]: ",
        "\nverdict: pass\n",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    let json: serde_json::Value = serde_json::from_str(&json_report(&server).unwrap()).unwrap();
    assert_eq!(json["sessions"]["verdict"], "pass");
    assert_eq!(json["sessions"]["findings"]["set_aside"], 1);
    assert_eq!(json["agreement"]["holds"], true);
    assert_eq!(
        json["agreement"]["divergences"]["set_aside"][0],
        serde_json::json!({
            "trace": "001.jsonl",
            "requirement": "BASE-003",
            "detail": json["agreement"]["divergences"]["set_aside"][0]["detail"],
            "party": "client",
        })
    );

    // A client under test: the same words with the ends the other way round.
    let client = outcome_for(
        Party::Client,
        true,
        vec![session(Party::Client, SERVER_BREAKS_PING_001)],
        &Baseline::default(),
    );
    let text = human(&client, false);
    for needle in [
        "Viewed for the client: a finding tagged [server] is held against the suite's own test \
         server and does not count",
        "subject: the client; the suite's own test server is the other end.",
        "failing clauses held against the suite's own test server (shown, and not counted):\n  \
         PING-001 on 001.jsonl [server]: ",
        "agreement: holds\n",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    // The same session read for the client whose finding it is.
    let own = outcome_for(
        Party::Client,
        true,
        vec![session(Party::Client, CLIENT_BREAKS_BASE_003)],
        &Baseline::default(),
    );
    let text = human(&own, false);
    assert!(text.contains("agreement: does not hold"), "{text}");
    assert!(
        text.contains("  BASE-003 on 001.jsonl [client]: "),
        "{text}"
    );
}

/// The validator's renderings follow the view: a failure set aside is a passing
/// case in `JUnit` and a note that is not counted in SARIF.
#[test]
fn junit_and_sarif_set_the_other_ends_findings_aside() {
    let server = outcome(true, vec![judged_client_fault()], &Baseline::default());
    let xml = junit_report(&server);
    assert!(xml.contains(r#"failures="0""#), "{xml}");
    assert!(xml.contains("set aside"), "{xml}");
    let log: serde_json::Value = serde_json::from_str(&sarif_report(&server)).unwrap();
    let aside = log["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["ruleId"] == "BASE-003")
        .unwrap();
    assert_eq!(aside["level"], "note", "{aside}");
    assert_eq!(aside["properties"]["counted"], false, "{aside}");
}

#[test]
fn the_fixture_note_names_a_few_of_each_kind_and_counts_the_rest() {
    let mut run = run_outcome(false);
    run.scenarios = vec![
        scenario("a", &[("FAILURE", Some("x"))]),
        scenario("b", &[("SUCCESS", None)]),
    ];
    assert!(
        !human(&run, false).contains("note: "),
        "none without evidence"
    );
    run.fixtures = Some(Fixtures {
        unlisted: Unlisted {
            tools: (1..=6).map(|n| format!("t{n}")).collect(),
            prompts: vec!["p".to_owned()],
            resources: vec![],
        },
        sessions: 3,
        judged_sessions: 7,
    });
    let text = human(&run, false);
    let note = "note: 1 of the suite's 2 scenario(s) failed. In 3 of the 7 recorded sessions the \
                suite asked for a tool, prompt or resource that none of this server's own list \
                answers in this run includes:\n  tools (6): t1, t2, t3, t4 and 2 more\n  \
                prompts (1): p\nThe pinned suite asks for these by fixed names";
    assert!(text.contains(note), "{text}");
    assert!(
        !text.contains("  resources ("),
        "a kind with none is left out: {text}"
    );
    let json: serde_json::Value = serde_json::from_str(&json_report(&run).unwrap()).unwrap();
    assert_eq!(
        json["suite"]["fixtures"]["unlisted"]["tools"],
        serde_json::json!(["t1", "t2", "t3", "t4", "t5", "t6"]),
        "the report has every name"
    );
    assert_eq!(json["suite"]["fixtures"]["failed_scenarios"], 1);
    assert_eq!(note::some_names(&["a".to_owned(), "b".to_owned()]), "a, b");
    assert_eq!(note::some_names(&[]), "");
}

#[test]
fn stderr_says_the_verdicts_where_the_report_on_stdout_holds_the_rest() {
    let args = |format, output: Option<&str>| ReportArgs {
        format,
        output: output.map(std::path::PathBuf::from),
        all: false,
    };
    assert!(report_on_stdout_carries_all(&args(Format::Human, None)));
    assert!(report_on_stdout_carries_all(&args(Format::Json, None)));
    assert!(!report_on_stdout_carries_all(&args(Format::Junit, None)));
    assert!(!report_on_stdout_carries_all(&args(Format::Sarif, None)));
    assert!(!report_on_stdout_carries_all(&args(
        Format::Human,
        Some("r")
    )));
    assert!(!report_on_stdout_carries_all(&args(
        Format::Json,
        Some("r")
    )));

    let mut text = String::new();
    let run = run_outcome(true);
    clause_verdict(&mut text, &run);
    agreement_verdict(&mut text, &run);
    assert_eq!(
        text,
        "clause verdict (server findings only) over 1 judged session(s): fail\n\
         agreement: does not hold (1 unexplained, 0 stale baseline entries; the report says \
         which)\n"
    );
    let mut text = String::new();
    let client_fault = outcome(true, vec![judged_client_fault()], &Baseline::default());
    clause_verdict(&mut text, &client_fault);
    agreement_verdict(&mut text, &client_fault);
    assert_eq!(
        text,
        "clause verdict (server findings only) over 1 judged session(s): pass \u{2014} 1 client \
         finding set aside\nagreement: holds\n"
    );
}

#[test]
fn the_json_report_carries_every_part() {
    let outcome = run_outcome(true);
    let report: serde_json::Value = serde_json::from_str(&json_report(&outcome).unwrap()).unwrap();
    assert_eq!(report["subject"], "http://localhost:3000/mcp");
    assert_eq!(report["work_dir"], "/w");
    assert_eq!(report["suite"]["version"], suite::VERSION);
    assert_eq!(report["suite"]["package"], suite::PACKAGE);
    assert_eq!(report["suite"]["spec_version"], "2025-11-25");
    assert_eq!(report["suite"]["passed"], true);
    assert_eq!(report["suite"]["summary"]["checks"], 4);
    assert_eq!(
        report["suite"]["scenarios"][1]["checks"][1]["errorMessage"],
        "slow"
    );
    assert_eq!(
        report["suite"]["not_run"][0]["scenario"],
        "dns-rebinding-protection"
    );
    assert_eq!(report["sessions"]["verdict"], "fail");
    assert_eq!(report["sessions"]["party"], "server");
    assert_eq!(
        report["sessions"]["findings"],
        serde_json::json!({"counted": 1, "set_aside": 0, "unattributed": 0})
    );
    assert_eq!(report["agreement"]["party"], "server");
    assert_eq!(
        report["sessions"]["traces"][0]["trace"],
        "/w/traces/001.jsonl"
    );
    assert_eq!(
        report["sessions"]["traces"][0]["reports"][0]["revision"],
        "2025-11-25"
    );
    assert_eq!(
        report["sessions"]["unjudged"][0]["trace"],
        "/w/traces/002.jsonl"
    );
    assert_eq!(report["sessions"]["unjudged"][0]["name"], "002.jsonl");
    assert_eq!(report["sessions"]["unjudged"][0]["class"], "informational");
    assert!(report["sessions"]["totals"]["fail"].as_u64().unwrap() >= 1);
    assert_eq!(report["agreement"]["baseline"], serde_json::Value::Null);
    assert_eq!(report["agreement"]["holds"], false);
    assert_eq!(
        report["agreement"]["divergences"]["unexplained"][0]["trace"],
        "001.jsonl"
    );
    assert_eq!(report["verdict"], "fail");
}

#[test]
fn junit_and_sarif_are_the_validators_renderings_of_the_sessions() {
    let outcome = run_outcome(true);
    let xml = junit_report(&outcome);
    assert!(
        xml.contains(r#"<testsuite name="/w/traces/001.jsonl (2025-11-25)""#),
        "{xml}"
    );
    // A session that held nothing to judge is no error in a CI report.
    assert!(
        !xml.contains("errors=") && !xml.contains("002.jsonl"),
        "{xml}"
    );
    let log: serde_json::Value = serde_json::from_str(&sarif_report(&outcome)).unwrap();
    assert_eq!(
        log["runs"][0]["invocations"][0]["executionSuccessful"], true,
        "informational: the run judged everything it could"
    );
    // One the run could not judge is.
    let mut broken = run_outcome(true);
    broken.judging.unjudged[0].class = crate::judging::Class::Error;
    let xml = junit_report(&broken);
    assert!(
        xml.contains(r#"errors="1""#) && xml.contains("002.jsonl"),
        "the unjudged session is listed: {xml}"
    );
    let log: serde_json::Value = serde_json::from_str(&sarif_report(&broken)).unwrap();
    let results = log["runs"][0]["results"].as_array().unwrap();
    assert!(
        results.iter().any(|result| result["ruleId"] == "PING-001"),
        "{log:#}"
    );
    assert_eq!(
        log["runs"][0]["invocations"][0]["executionSuccessful"], false,
        "the unjudged session is reported"
    );
}

/// The run's verdict is `pass` (exit 0), `fail` (exit 1: findings) or
/// `incomplete` (exit 2: it could not judge what it recorded), and the report
/// says which sessions, and why, in every form a person or a pipeline reads.
#[test]
fn a_run_that_could_not_judge_a_session_reads_incomplete_and_says_which() {
    let mut run = run_outcome(false);
    run.suite_passed = true;
    let json = |run: &Outcome| -> serde_json::Value {
        serde_json::from_str(&json_report(run).unwrap()).unwrap()
    };
    assert_eq!(json(&run)["verdict"], "pass");
    let text = human(&run, false);
    assert!(
        text.contains("not judged (informational): the server sent no message"),
        "{text}"
    );
    assert!(text.contains("1 not judged"), "{text}");

    run.judging.unjudged[0].class = crate::judging::Class::Error;
    let report = json(&run);
    assert_eq!(report["verdict"], "incomplete");
    assert_eq!(report["sessions"]["unjudged"][0]["class"], "error");
    let text = human(&run, false);
    assert!(
        text.contains("not judged (error): the server sent no message"),
        "{text}"
    );
    assert!(text.contains("1 not judged (1 error)"), "{text}");
    assert!(text.ends_with("verdict: incomplete\n"), "{text}");

    let mut stale = run_outcome(false);
    stale.judging.stale_expected = vec!["sse-retry".to_owned()];
    let text = human(&stale, false);
    assert!(
        text.contains("expected to go unjudged for its revision, and did not: sse-retry"),
        "{text}"
    );
    assert_eq!(
        json(&stale)["sessions"]["stale_expected"],
        serde_json::json!(["sse-retry"])
    );
    assert_eq!(json(&stale)["verdict"], "incomplete");
}

#[test]
fn a_report_is_written_where_asked() {
    let dir = std::env::temp_dir().join(format!("mcp-conformance-write-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("report.json");
    let args = ReportArgs {
        format: Format::Json,
        output: Some(file.clone()),
        all: false,
    };
    assert!(write(&run_outcome(false), &args));
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.ends_with("}\n"), "{written}");
    let unwritable = ReportArgs {
        output: Some(dir.join("missing/report.json")),
        ..args
    };
    assert!(!write(&run_outcome(false), &unwritable));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn counts_read_as_the_suite_writes_them() {
    let by_status = BTreeMap::from([("FAILURE".to_owned(), 2), ("SUCCESS".to_owned(), 38)]);
    assert_eq!(counts(&by_status), "2 FAILURE, 38 SUCCESS");
    assert_eq!(counts(&BTreeMap::new()), "");
}

/// A writer whose every write fails with its error kind.
struct Failing(std::io::ErrorKind);

impl std::io::Write for Failing {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(self.0.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_reader_that_closed_the_pipe_has_the_report_and_any_other_failure_loses_it() {
    let mut out = Vec::new();
    assert!(emit_to(&mut out, "report\n"));
    assert_eq!(out, b"report\n");
    assert!(emit_to(
        &mut Failing(std::io::ErrorKind::BrokenPipe),
        "report\n"
    ));
    assert!(!emit_to(
        &mut Failing(std::io::ErrorKind::PermissionDenied),
        "report\n"
    ));
}

#[test]
fn a_clauses_findings_past_the_limit_are_counted_unless_all_are_asked_for() {
    // Every ping reuses the initialize request's id: one finding each.
    let mut trace = String::from(concat!(
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#,
        "\n",
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#,
        "\n",
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
        "\n"
    ));
    let pings = MAX_FINDINGS + 3;
    for n in 0..pings {
        let seq = 3 + 2 * n;
        write!(
            trace,
            "{{\"seq\":{seq},\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}}}}\n{{\"seq\":{},\"direction\":\"server-to-client\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}}}\n",
            seq + 1
        )
        .unwrap();
    }
    let judgment = mcp_trace_validator::judge::judge(&trace).unwrap();
    let mut outcome = run_outcome(true);
    outcome.judging.judged[0].events = judgment.events;
    outcome.judging.judged[0].reports = judgment.reports;
    let last = r#"seq 47: request "ping" reuses id 1"#;
    let limited = human(&outcome, false);
    assert!(limited.contains("        … and 3 more\n"), "{limited}");
    assert!(!limited.contains(last), "{limited}");
    let all = human(&outcome, true);
    assert!(all.contains(last), "{all}");
    assert!(!all.contains("… and"), "{all}");
}
