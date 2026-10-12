// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A run over several traces accounts for the ones it could not judge, in
//! every format.
//!
//! The exit status always did (`3` for a malformed trace among good ones), but
//! the report said `overall verdict: pass`, the JSON `"verdict": "pass"`, and
//! the `JUnit` and SARIF files left the trace out entirely — so the document CI
//! uploads, the one a person reads, disagreed with the exit status that failed
//! the job.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mcp_trace_validator::report::JSON_SCHEMA;
use serde_json::Value;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn corpus(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
        .to_str()
        .unwrap()
        .to_owned()
}

/// A malformed trace in its own temp directory; the caller removes the directory.
fn broken(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp-tv-runs-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broken.jsonl");
    std::fs::write(&path, "not json\n").unwrap();
    path
}

#[test]
fn an_unjudged_trace_makes_the_human_verdict_incomplete_and_is_listed() {
    let path = broken("human");
    let bad = path.to_str().unwrap();
    let good = corpus("good/stdio-minimal-init.jsonl");
    let output = run(&["validate", &good, bad, &good]);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let text = stdout(&output);
    assert!(
        text.contains(&format!(
            "==> {bad} <==\nnot judged: {bad}: malformed trace: line 1:"
        )),
        "{text}"
    );
    assert!(
        text.ends_with(
            "3 traces: 2 pass, 0 pass-with-warnings, 0 fail, 1 not judged (see the errors above)\n\
             overall verdict: incomplete\n"
        ),
        "{text}"
    );
}

#[test]
fn an_unjudged_trace_is_listed_with_its_error_in_the_json() {
    let path = broken("json");
    let bad = path.to_str().unwrap();
    let failing = corpus("violations/base-003-request-id-reuse.jsonl");
    let output = run(&["validate", "--format", "json", &failing, bad]);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let document: Value = serde_json::from_str(&stdout(&output)).unwrap();
    // Incomplete outranks the judged trace's fail, as exit 3 outranks 1.
    assert_eq!(document["verdict"], "incomplete");
    assert_eq!(document["unjudged"], 1);
    assert_eq!(document["unjudged_traces"][0]["trace"], bad);
    let error = document["unjudged_traces"][0]["error"].as_str().unwrap();
    assert!(
        error.starts_with(&format!("{bad}: malformed trace: line 1:")),
        "{error}"
    );

    let schema: Value = serde_json::from_str(JSON_SCHEMA).unwrap();
    let validator = jsonschema::draft202012::new(&schema).unwrap();
    assert!(validator.is_valid(&document), "{document}");

    // Every trace judged: no list, and the verdict is the judged ones'.
    let judged: Value = serde_json::from_str(&stdout(&run(&[
        "validate", "--format", "json", &failing, &failing,
    ])))
    .unwrap();
    assert_eq!(judged["verdict"], "fail");
    assert!(judged.get("unjudged_traces").is_none(), "{judged}");
}

#[test]
fn an_unjudged_trace_is_an_error_test_case_in_junit() {
    let path = broken("junit");
    let bad = path.to_str().unwrap();
    let good = corpus("good/stdio-minimal-init.jsonl");
    let output = run(&["validate", "--format", "junit", &good, bad]);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let xml = stdout(&output);
    assert!(xml.contains(r#" errors="1">"#), "{xml}");
    assert!(
        xml.contains(&format!(
            r#"<testsuite name="{bad}" tests="1" failures="0" errors="1" skipped="0">"#
        )),
        "{xml}"
    );
    assert!(
        xml.contains(&format!(
            r#"<error message="{bad}: malformed trace: line 1:"#
        )),
        "{xml}"
    );
}

#[test]
fn an_unjudged_trace_fails_the_sarif_invocation() {
    let path = broken("sarif");
    let bad = path.to_str().unwrap();
    let good = corpus("good/stdio-minimal-init.jsonl");
    let output = run(&["validate", "--format", "sarif", &good, bad]);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let log: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let invocation = &log["runs"][0]["invocations"][0];
    assert_eq!(invocation["executionSuccessful"], false, "{invocation}");
    let notification = &invocation["toolExecutionNotifications"][0];
    assert_eq!(notification["level"], "error");
    assert!(
        notification["message"]["text"]
            .as_str()
            .unwrap()
            .contains("malformed trace: line 1:"),
        "{notification}"
    );
    let uri = notification["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
        .as_str()
        .unwrap();
    assert!(
        uri.starts_with("file://") && uri.ends_with("/broken.jsonl"),
        "{notification}"
    );
}
