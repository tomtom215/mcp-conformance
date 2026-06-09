// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Integration tests for the `mcp-trace-validator` binary.
//!
//! The exit codes are a documented, stable interface (`0` pass / `1` findings / `2`
//! invocation problem / `3` malformed trace) that CI integrations script against —
//! so they are pinned here by executing the real binary, not by unit-testing
//! internals.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
}

fn corpus(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
}

fn run(args: &[&str]) -> Output {
    let mut command = binary();
    command.args(args);
    command.output().expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn passing_trace_exits_zero_with_pass_verdict() {
    let output = run(&[
        "validate",
        corpus("good/stdio-full-session.jsonl").to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        stdout(&output).contains("verdict: pass"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn violating_trace_exits_one_and_names_the_requirement() {
    let output = run(&[
        "validate",
        corpus("violations/life-001-first-message-not-initialize.jsonl")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("FAIL  LIFE-001"), "{text}");
    assert!(text.contains("verdict: fail"), "{text}");
}

#[test]
fn warnings_pass_by_default_and_fail_under_strict() {
    let trace = corpus("violations/life-004-client-request-before-init-response.jsonl");
    let lenient = run(&["validate", trace.to_str().unwrap()]);
    assert_eq!(lenient.status.code(), Some(0), "{lenient:?}");
    assert!(stdout(&lenient).contains("verdict: pass-with-warnings"));

    let strict = run(&["validate", trace.to_str().unwrap(), "--strict"]);
    assert_eq!(strict.status.code(), Some(1), "{strict:?}");
}

#[test]
fn json_format_emits_a_parseable_report() {
    let output = run(&[
        "validate",
        corpus("violations/base-008-jsonrpc-version.jsonl")
            .to_str()
            .unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(report["revision"], "2025-11-25");
    assert_eq!(report["totals"]["fail"], 1);
}

#[test]
fn stdin_dash_reads_the_trace_from_stdin() {
    let trace = std::fs::read(corpus("good/stdio-minimal-init.jsonl")).unwrap();
    let mut child = binary()
        .args(["validate", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&trace).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stdout(&output).contains("verdict: pass"));
}

#[test]
fn malformed_trace_exits_three() {
    let path =
        std::env::temp_dir().join(format!("mcp-tv-cli-malformed-{}.jsonl", std::process::id()));
    std::fs::write(&path, "this is not json\n").unwrap();
    let output = run(&["validate", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("malformed trace"), "{stderr}");
}

#[test]
fn unreadable_inputs_exit_two() {
    let missing_trace = run(&["validate", "/nonexistent/trace.jsonl"]);
    assert_eq!(missing_trace.status.code(), Some(2), "{missing_trace:?}");

    let missing_registry = run(&[
        "validate",
        corpus("good/stdio-minimal-init.jsonl").to_str().unwrap(),
        "--registry",
        "/nonexistent/registry.json",
    ]);
    assert_eq!(
        missing_registry.status.code(),
        Some(2),
        "{missing_registry:?}"
    );
}

#[test]
fn registry_referencing_unknown_checks_exits_two() {
    let path =
        std::env::temp_dir().join(format!("mcp-tv-cli-registry-{}.json", std::process::id()));
    std::fs::write(
        &path,
        r#"{"revision":"2025-11-25","requirements":[
            {"id":"FUTR-001","level":"MUST","actor":"both",
             "source":{"section":"future#x","quote":"MUST do future things"},
             "checks":["future.not-built-yet"]}]}"#,
    )
    .unwrap();
    let output = run(&[
        "validate",
        corpus("good/stdio-minimal-init.jsonl").to_str().unwrap(),
        "--registry",
        path.to_str().unwrap(),
    ]);
    std::fs::remove_file(&path).ok();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(stdout(&output).contains("UNSUP"), "{}", stdout(&output));
}

#[test]
fn requirements_lists_the_registry_in_both_formats() {
    let human = run(&["requirements"]);
    assert_eq!(human.status.code(), Some(0));
    let text = stdout(&human);
    assert!(text.contains("LIFE-001"), "{text}");
    assert!(
        text.contains("checks: lifecycle.first-interaction-initialize"),
        "{text}"
    );
    assert!(text.contains("excluded"), "{text}");

    let json = run(&["requirements", "--format", "json"]);
    assert_eq!(json.status.code(), Some(0));
    let registry: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(registry["revision"], "2025-11-25");
}
