// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use mcp_conformance_cli::agreement::Baseline;
use mcp_conformance_cli::suite::Check;

use super::*;
use crate::run::tests::{judged, outcome};

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
        "==> /w/traces/001.jsonl <==",
        "BASE-003",
        "==> /w/traces/002.jsonl <==\nnot judged: the server sent no message",
        "2 sessions: 0 pass, 0 pass-with-warnings, 1 fail, 1 not judged; clause verdict: fail",
        "== Agreement ==",
        "divergence baseline: none",
        "agreement: does not hold",
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
    assert!(
        xml.contains(r#"errors="1""#),
        "the unjudged session is listed: {xml}"
    );
    let log: serde_json::Value = serde_json::from_str(&sarif_report(&outcome)).unwrap();
    let results = log["runs"][0]["results"].as_array().unwrap();
    assert!(
        results.iter().any(|result| result["ruleId"] == "BASE-003"),
        "{log:#}"
    );
    assert_eq!(
        log["runs"][0]["invocations"][0]["executionSuccessful"], false,
        "the unjudged session is reported"
    );
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
