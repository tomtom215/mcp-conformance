// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `validate --baseline` and `--write-baseline` (ADR-0025), by running the
//! binary on a real recording: the corpus capture of the TypeScript SDK's
//! conformance server, whose eight tools each draw a `BASE-086` warning.
//!
//! What the author of a server in that state needs is pinned end to end: write
//! the findings down once; then a run fails on a finding that is new, a stale
//! entry or an expired one, and on nothing it has accepted — while every
//! accepted finding stays in every format.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mcp_trace_validator::baseline::{Baseline, GENERATED_REASON};
use mcp_trace_validator::report::JSON_SCHEMA;
use serde_json::{Value, json};

const SDK_TRACE: &str = "captured/sdk-typescript-2025-11-25.jsonl";

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn corpus(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
        .to_str()
        .unwrap()
        .to_owned()
}

/// A scratch directory of this test's own.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp-tv-baseline-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_str().unwrap().to_owned()
}

/// The SDK recording with `change` applied to its `tools/list` tools.
fn sdk_variant(dir: &Path, name: &str, change: impl Fn(&mut Vec<Value>)) -> String {
    let text = std::fs::read_to_string(corpus(SDK_TRACE)).unwrap();
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let mut event: Value = serde_json::from_str(line).unwrap();
            if let Some(tools) = event["payload"]["result"]["tools"].as_array_mut() {
                change(tools);
            }
            event.to_string()
        })
        .collect();
    let target = path(dir, name);
    std::fs::write(&target, lines.join("\n") + "\n").unwrap();
    target
}

fn with_new_tool(dir: &Path) -> String {
    sdk_variant(dir, "new-tool.jsonl", |tools| {
        tools.push(json!({
            "name": "brand_new_tool",
            "inputSchema": {"$schema": "http://json-schema.org/draft-07/schema#", "type": "object"},
        }));
    })
}

fn with_a_tool_fixed(dir: &Path) -> String {
    sdk_variant(dir, "fixed-tool.jsonl", |tools| {
        for tool in tools
            .iter_mut()
            .filter(|tool| tool["name"] == "test_sampling")
        {
            tool["inputSchema"]["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
        }
    })
}

/// Writes a baseline of the SDK recording into `dir`, returning its path.
fn baseline_of_sdk(dir: &Path) -> String {
    let file = path(dir, "baseline.json");
    let written = run(&["validate", &corpus(SDK_TRACE), "--write-baseline", &file]);
    assert_eq!(code(&written), 0, "{}", stderr(&written));
    file
}

fn validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(JSON_SCHEMA).unwrap();
    jsonschema::draft202012::new(&schema).unwrap()
}

fn assert_schema_valid(document: &Value) {
    let errors: Vec<String> = validator()
        .iter_errors(document)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

fn json_of(args: &[&str]) -> (Value, i32) {
    let output = run(args);
    (
        serde_json::from_str(&stdout(&output)).unwrap(),
        code(&output),
    )
}

#[test]
fn what_is_written_is_what_the_next_run_prints_and_a_clean_run_follows() {
    let dir = scratch("roundtrip");
    let file = path(&dir, "baseline.json");
    let trace = corpus(SDK_TRACE);
    let writing = run(&["validate", &trace, "--write-baseline", &file]);
    assert_eq!(code(&writing), 0, "{}", stderr(&writing));
    assert!(
        stderr(&writing).contains("wrote 8 entries to"),
        "{}",
        stderr(&writing)
    );
    assert!(
        stderr(&writing).contains("generated reason"),
        "{}",
        stderr(&writing)
    );

    let baseline = Baseline::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(baseline.entries().len(), 8);
    assert!(
        baseline
            .entries()
            .iter()
            .all(|entry| entry.reason == GENERATED_REASON)
    );

    // The run that writes prints the report the next run prints.
    let reading = run(&["validate", &trace, "--baseline", &file]);
    assert_eq!(code(&reading), 0);
    assert_eq!(stdout(&writing), stdout(&reading));
    let text = stdout(&reading);
    assert!(
        text.contains("verdict (baseline applied): pass\n"),
        "{text}"
    );
    assert!(
        text.contains("baseline: 8 findings accepted, 0 not accepted; 8 of the accepted carry"),
        "{text}"
    );
    assert_eq!(
        text.matches("[accepted]").count(),
        8,
        "every accepted finding is still listed"
    );

    // `--strict` fails on those warnings without the baseline, and not with it.
    assert_eq!(code(&run(&["validate", &trace, "--strict"])), 1);
    assert_eq!(
        code(&run(&["validate", &trace, "--strict", "--baseline", &file])),
        0
    );
}

#[test]
fn a_new_finding_beside_accepted_ones_decides_the_verdict_and_the_exit_status() {
    let dir = scratch("new");
    let file = baseline_of_sdk(&dir);
    let trace = with_new_tool(&dir);
    let output = run(&["validate", &trace, "--baseline", &file, "--strict"]);
    assert_eq!(code(&output), 1, "{}", stdout(&output));
    let text = stdout(&output);
    assert!(
        text.contains("baseline: 8 findings accepted, 1 not accepted"),
        "{text}"
    );
    assert!(
        text.contains("verdict (baseline applied): pass-with-warnings"),
        "{text}"
    );
    let new_lines = text
        .lines()
        .filter(|line| line.contains("brand_new_tool") && !line.contains("[accepted]"))
        .count();
    assert_eq!(new_lines, 1, "{text}");
    // Without --strict a warning is not a failure, as ever.
    assert_eq!(code(&run(&["validate", &trace, "--baseline", &file])), 0);
}

#[test]
fn a_stale_entry_fails_the_run_unless_allowed_and_is_listed_in_the_terminal_and_in_json() {
    let dir = scratch("stale");
    let file = baseline_of_sdk(&dir);
    let trace = with_a_tool_fixed(&dir);

    let human = run(&["validate", &trace, "--baseline", &file]);
    assert_eq!(code(&human), 1, "{}", stdout(&human));
    assert!(
        stdout(&human).contains("1 entry matches no finding of this run"),
        "{}",
        stdout(&human)
    );
    assert!(stdout(&human).contains("test_sampling"));
    assert!(
        stderr(&human).contains("this run exits 1"),
        "{}",
        stderr(&human)
    );
    assert!(
        stdout(&human).contains("verdict (baseline applied): pass\n"),
        "the verdict is the findings'"
    );

    let allowed = run(&[
        "validate",
        &trace,
        "--baseline",
        &file,
        "--allow-stale-baseline",
    ]);
    assert_eq!(code(&allowed), 0);
    assert!(
        stdout(&allowed).contains("stale entries are allowed"),
        "{}",
        stdout(&allowed)
    );

    let (json, status) = json_of(&["validate", &trace, "--baseline", &file, "--format", "json"]);
    assert_eq!(status, 1);
    assert_eq!(json["baseline"]["accepted"], 7);
    assert_eq!(json["baseline"]["unaccepted"], 0);
    assert_eq!(json["baseline"]["stale"].as_array().unwrap().len(), 1);
    assert!(
        json["baseline"]["stale"][0]["fingerprint"]
            .as_str()
            .unwrap()
            .contains("test_sampling")
    );
    assert_schema_valid(&json);
}

#[test]
fn a_stale_entry_is_a_failing_case_a_notification_and_a_note_in_the_other_formats() {
    let dir = scratch("stale-formats");
    let file = baseline_of_sdk(&dir);
    let trace = with_a_tool_fixed(&dir);
    let junit = stdout(&run(&[
        "validate",
        &trace,
        "--baseline",
        &file,
        "--format",
        "junit",
    ]));
    assert!(
        junit.contains("<testsuite name=\"baseline\" tests=\"1\" failures=\"1\""),
        "{junit}"
    );
    let junit_allowed = stdout(&run(&[
        "validate",
        &trace,
        "--baseline",
        &file,
        "--allow-stale-baseline",
        "--format",
        "junit",
    ]));
    assert!(
        junit_allowed.contains("<testsuite name=\"baseline\" tests=\"1\" failures=\"0\""),
        "{junit_allowed}"
    );

    let sarif: Value = serde_json::from_str(&stdout(&run(&[
        "validate",
        &trace,
        "--baseline",
        &file,
        "--format",
        "sarif",
    ])))
    .unwrap();
    let notes = &sarif["runs"][0]["invocations"][0]["toolExecutionNotifications"];
    assert_eq!(notes[0]["properties"]["kind"], "baseline-stale");

    let html = stdout(&run(&[
        "validate",
        &trace,
        "--baseline",
        &file,
        "--format",
        "html",
    ]));
    assert!(
        html.contains("Baseline entries that did not do their job."),
        "no note in the page"
    );
}

#[test]
fn an_expired_entry_accepts_nothing_and_a_future_one_accepts() {
    let dir = scratch("expiry");
    let file = baseline_of_sdk(&dir);
    let trace = corpus(SDK_TRACE);
    let edit = |day: &str| {
        let mut baseline: Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        baseline["findings"][0]["expires"] = json!(day);
        baseline["findings"][0]["reason"] = json!("until the SDK ships 2020-12");
        let edited = path(&dir, &format!("expiring-{day}.json"));
        std::fs::write(&edited, baseline.to_string()).unwrap();
        edited
    };

    let expired = edit("2020-07-01");
    let output = run(&["validate", &trace, "--baseline", &expired, "--strict"]);
    assert_eq!(code(&output), 1, "{}", stdout(&output));
    let text = stdout(&output);
    assert!(
        text.contains("baseline: 7 findings accepted, 1 not accepted"),
        "{text}"
    );
    assert!(
        text.contains(
            "1 entry has expired and accepts nothing; the finding it matched counts again"
        ),
        "{text}"
    );
    assert!(text.contains("(expired 2020-07-01)"), "{text}");
    let (json, _) = json_of(&[
        "validate",
        &trace,
        "--baseline",
        &expired,
        "--format",
        "json",
    ]);
    assert_eq!(json["baseline"]["lapsed"][0]["expires"], "2020-07-01");
    assert!(
        json["baseline"].get("stale").is_none(),
        "it matched, so it is not stale"
    );
    assert_schema_valid(&json);

    let future = edit("2999-12-31");
    let output = run(&["validate", &trace, "--baseline", &future, "--strict"]);
    assert_eq!(code(&output), 0, "{}", stdout(&output));
    assert!(
        stdout(&output).contains("(accepted through 2999-12-31)"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_file_write_baseline_has_not_read_is_never_overwritten() {
    let dir = scratch("refresh");
    let file = baseline_of_sdk(&dir);
    let mut baseline: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    baseline["findings"][0]["reason"] = json!("reviewed: the SDK's generator");
    std::fs::write(&file, baseline.to_string()).unwrap();
    let before = std::fs::read(&file).unwrap();

    // Not the baseline in use: refused, and the file is as it was.
    let refused = run(&["validate", &corpus(SDK_TRACE), "--write-baseline", &file]);
    assert_eq!(code(&refused), 2);
    assert!(
        stderr(&refused).contains("will not overwrite a file it has not read"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert_eq!(
        stdout(&refused),
        "",
        "nothing is reported when the run is refused"
    );

    // A different file is not the one in use either.
    let other = path(&dir, "other.json");
    std::fs::write(&other, &before).unwrap();
    let refused = run(&[
        "validate",
        &corpus(SDK_TRACE),
        "--baseline",
        &other,
        "--write-baseline",
        &file,
    ]);
    assert_eq!(code(&refused), 2);
}

#[test]
fn a_refresh_keeps_reasons_adds_new_findings_and_drops_what_no_longer_occurs() {
    let dir = scratch("refresh-keeps");
    let file = baseline_of_sdk(&dir);
    let mut baseline: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    baseline["findings"][0]["reason"] = json!("reviewed: the SDK's generator");
    let reviewed = baseline["findings"][0]["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::write(&file, baseline.to_string()).unwrap();

    // The baseline in use, against a recording with another tool added.
    let trace = with_new_tool(&dir);
    let refreshed = run(&[
        "validate",
        &trace,
        "--baseline",
        &file,
        "--write-baseline",
        &file,
    ]);
    assert_eq!(code(&refreshed), 0, "{}", stderr(&refreshed));
    assert!(
        stderr(&refreshed).contains("wrote 9 entries"),
        "{}",
        stderr(&refreshed)
    );
    assert!(
        stderr(&refreshed).contains("8 kept with their reasons and expiry, 1 added"),
        "{}",
        stderr(&refreshed)
    );
    let after = Baseline::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let kept = after
        .entries()
        .iter()
        .find(|entry| entry.fingerprint == reviewed)
        .unwrap();
    assert_eq!(kept.reason, "reviewed: the SDK's generator");
    let added = after
        .entries()
        .iter()
        .find(|entry| entry.fingerprint.contains("brand_new_tool"))
        .unwrap();
    assert_eq!(added.reason, GENERATED_REASON);

    // Refreshing against the fixed recording — which also lacks the added tool —
    // drops both entries, and says so.
    let fixed = with_a_tool_fixed(&dir);
    let refreshed = run(&[
        "validate",
        &fixed,
        "--baseline",
        &file,
        "--write-baseline",
        &file,
    ]);
    assert!(
        stderr(&refreshed).contains("2 dropped for matching no finding"),
        "{}",
        stderr(&refreshed)
    );
    assert_eq!(code(&refreshed), 0);
}

#[test]
fn nothing_is_written_when_a_trace_could_not_be_judged() {
    let dir = scratch("unjudged");
    let file = path(&dir, "baseline.json");
    let malformed = path(&dir, "malformed.jsonl");
    std::fs::write(&malformed, "not json\n").unwrap();
    let output = run(&[
        "validate",
        &corpus(SDK_TRACE),
        &malformed,
        "--write-baseline",
        &file,
    ]);
    assert_eq!(code(&output), 3, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("wrote nothing"),
        "{}",
        stderr(&output)
    );
    assert!(
        !Path::new(&file).exists(),
        "a partial baseline would lose the unjudged trace's findings"
    );
}

#[test]
fn a_baseline_that_cannot_be_used_is_a_usage_error_naming_why() {
    let dir = scratch("bad");
    let trace = corpus(SDK_TRACE);
    let missing = run(&["validate", &trace, "--baseline", &path(&dir, "absent.json")]);
    assert_eq!(code(&missing), 2);
    assert!(
        stderr(&missing).contains("cannot read"),
        "{}",
        stderr(&missing)
    );

    let bad = path(&dir, "blank.json");
    std::fs::write(
        &bad,
        json!({"version": 1, "fingerprint_key": "mcpConformanceFinding/v2",
               "findings": [{"fingerprint": "A:1", "reason": "  "}]})
        .to_string(),
    )
    .unwrap();
    let blank = run(&["validate", &trace, "--baseline", &bad]);
    assert_eq!(code(&blank), 2);
    assert!(
        stderr(&blank).contains("baseline entry 1"),
        "{}",
        stderr(&blank)
    );
    assert!(stderr(&blank).contains("say why"), "{}", stderr(&blank));
    assert_eq!(stdout(&blank), "");

    let scheme = path(&dir, "scheme.json");
    std::fs::write(
        &scheme,
        json!({"version": 1, "fingerprint_key": "x/v9", "findings": []}).to_string(),
    )
    .unwrap();
    let wrong = run(&["validate", &trace, "--baseline", &scheme]);
    assert_eq!(code(&wrong), 2);
    assert!(
        stderr(&wrong).contains("this build computes"),
        "{}",
        stderr(&wrong)
    );

    let to_stdout = run(&["validate", &trace, "--write-baseline", "-"]);
    assert_eq!(code(&to_stdout), 2);
    let alone = run(&["validate", &trace, "--allow-stale-baseline"]);
    assert_eq!(code(&alone), 2, "it relaxes a baseline, so it needs one");
}

#[test]
fn several_traces_share_one_baseline_and_an_entry_is_stale_only_if_none_matches() {
    let dir = scratch("many");
    let file = baseline_of_sdk(&dir);
    let sdk = corpus(SDK_TRACE);
    let clean = corpus("good/stdio-full-session.jsonl");
    let (json, status) = json_of(&[
        "validate",
        &sdk,
        &clean,
        "--baseline",
        &file,
        "--format",
        "json",
    ]);
    assert_eq!(status, 0, "the clean trace does not make the entries stale");
    assert_eq!(json["baseline"]["accepted"], 8);
    assert!(json["baseline"].get("stale").is_none());
    assert!(
        json["traces"][1]["report"]["baseline"].is_object(),
        "each trace's own counts"
    );
    assert_schema_valid(&json);
    // Against the clean trace alone, every entry is stale.
    let alone = run(&["validate", &clean, "--baseline", &file]);
    assert_eq!(code(&alone), 1);
    // The same file accepts the same finding in a second recording of it.
    let again = path(&dir, "again.jsonl");
    std::fs::copy(&sdk, &again).unwrap();
    let (json, status) = json_of(&[
        "validate",
        &sdk,
        &again,
        "--baseline",
        &file,
        "--format",
        "json",
    ]);
    assert_eq!(status, 0);
    assert_eq!(json["baseline"]["accepted"], 16);
    let human = run(&["validate", &sdk, &again, "--baseline", &file]);
    assert!(
        stdout(&human).contains("2 traces: 2 pass, 0 pass-with-warnings, 0 fail"),
        "{}",
        stdout(&human)
    );
    let junit = stdout(&run(&[
        "validate",
        &sdk,
        &again,
        "--baseline",
        &file,
        "--format",
        "junit",
    ]));
    assert!(!junit.contains("name=\"baseline\""), "no notes, no suite");
}

#[test]
fn a_run_judged_at_two_revisions_applies_the_baseline_to_each() {
    let dir = scratch("revisions");
    let file = path(&dir, "baseline.json");
    let trace = corpus(SDK_TRACE);
    let both = ["--revision", "2025-11-25", "--revision", "2026-07-28"];
    let mut write = vec!["validate", &trace];
    write.extend(both);
    write.extend(["--write-baseline", &file]);
    let writing = run(&write);
    assert_eq!(code(&writing), 0, "{}", stderr(&writing));
    let text = stdout(&writing);
    assert!(
        text.contains("overall verdict (baseline applied): pass\n"),
        "{text}"
    );
    assert!(
        text.contains("(baseline applied)"),
        "per-revision lines say so: {text}"
    );
    let mut read = vec!["validate", &trace];
    read.extend(both);
    read.extend(["--baseline", &file, "--format", "json"]);
    let output = run(&read);
    assert_eq!(code(&output), 0);
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["verdict"], "pass");
    assert!(json["baseline"]["accepted"].as_u64().unwrap() >= 8);
    assert_eq!(json["baseline"]["unaccepted"], 0);
    assert_schema_valid(&json);
}

#[test]
fn a_party_view_and_a_baseline_each_take_findings_out_of_the_verdict() {
    let dir = scratch("party");
    let file = baseline_of_sdk(&dir);
    let output = run(&[
        "validate",
        &corpus(SDK_TRACE),
        "--baseline",
        &file,
        "--party",
        "server",
    ]);
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).contains("verdict (server findings only, baseline applied): pass\n"),
        "{}",
        stdout(&output)
    );
    // A baseline written under a view records what counts under it: the
    // client's view sets the server's eight warnings aside.
    let file = path(&dir, "client.json");
    let output = run(&[
        "validate",
        &corpus(SDK_TRACE),
        "--party",
        "client",
        "--write-baseline",
        &file,
    ]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        stderr(&output).contains("wrote 0 entries"),
        "{}",
        stderr(&output)
    );
    // With no entry there is no generated reason to replace, so the line ends
    // at the file's name.
    assert_eq!(
        stderr(&output).lines().last().unwrap(),
        format!("wrote 0 entries to {file}")
    );
}

#[test]
fn the_baseline_in_use_may_be_refreshed_under_another_spelling_of_its_path() {
    let dir = scratch("spelling");
    baseline_of_sdk(&dir);
    // `./baseline.json` and `baseline.json` are one file, so the refresh keeps
    // the reasons it has read; it is not refused as overwriting a stranger.
    let refreshed = Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(["validate", &corpus(SDK_TRACE)])
        .args(["--baseline", "./baseline.json"])
        .args(["--write-baseline", "baseline.json"])
        .current_dir(&dir)
        .output()
        .expect("binary runs");
    assert_eq!(code(&refreshed), 0, "{}", stderr(&refreshed));
    assert!(
        stderr(&refreshed).contains("wrote 8 entries to baseline.json (8 kept"),
        "{}",
        stderr(&refreshed)
    );

    // A baseline that is not there is no reading of the file being written:
    // the target is still refused, for the reasons it holds.
    let other = Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(["validate", &corpus(SDK_TRACE)])
        .args(["--baseline", "missing.json"])
        .args(["--write-baseline", "baseline.json"])
        .current_dir(&dir)
        .output()
        .expect("binary runs");
    assert_eq!(code(&other), 2, "{}", stderr(&other));
    assert!(
        stderr(&other).contains("baseline.json exists, and --write-baseline will not overwrite"),
        "{}",
        stderr(&other)
    );
}

#[test]
fn each_trace_numbers_its_own_findings_so_two_alike_write_one_entry_apiece() {
    let dir = scratch("alike");
    let copy = sdk_variant(&dir, "copy.jsonl", |_| {});
    let file = path(&dir, "baseline.json");
    // The same eight findings in two traces are eight entries, not sixteen:
    // an entry's number counts within the trace it was found in.
    let written = run(&[
        "validate",
        &corpus(SDK_TRACE),
        &copy,
        "--write-baseline",
        &file,
    ]);
    assert_eq!(code(&written), 0, "{}", stderr(&written));
    assert!(
        stderr(&written).contains("wrote 8 entries to"),
        "{}",
        stderr(&written)
    );
    let baseline = Baseline::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(baseline.entries().len(), 8);
    // And it is the baseline one of them alone would have written.
    let alone = baseline_of_sdk(&scratch("alike-alone"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        std::fs::read_to_string(alone).unwrap()
    );
}

#[test]
fn without_a_baseline_the_report_has_no_trace_of_one() {
    let trace = corpus(SDK_TRACE);
    let (json, _) = json_of(&["validate", &trace, "--format", "json"]);
    assert!(json.get("baseline").is_none());
    assert!(
        json["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row["findings"].as_array().into_iter().flatten())
            .all(|finding| finding.get("baselined").is_none())
    );
    let text = stdout(&run(&["validate", &trace]));
    assert!(
        !text.contains("[accepted]") && !text.contains("baseline"),
        "{text}"
    );
}

#[test]
fn a_baselined_json_report_marks_each_finding_and_validates_against_the_schema() {
    let dir = scratch("json");
    let file = baseline_of_sdk(&dir);
    let (json, status) = json_of(&[
        "validate",
        &corpus(SDK_TRACE),
        "--baseline",
        &file,
        "--format",
        "json",
    ]);
    assert_eq!(status, 0);
    assert_eq!(json["verdict"], "pass");
    assert_eq!(
        json["totals"]["warn"], 1,
        "the totals are the trace's facts"
    );
    let row = json["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "BASE-086")
        .unwrap();
    assert_eq!(row["outcome"], "warn");
    assert_eq!(row["findings"].as_array().unwrap().len(), 8);
    assert!(
        row["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["baselined"]["reason"] == GENERATED_REASON)
    );
    assert_eq!(
        json["baseline"],
        json!({"accepted": 8, "unaccepted": 0, "unreviewed": 8})
    );
    assert_schema_valid(&json);
}
