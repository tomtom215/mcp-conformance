// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `validate --format html`: the page the CLI writes is the golden page, and
//! the exit status is the one every other format gives.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs the binary from the repository root, so trace paths read as the
/// golden pages name them.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("binary runs")
}

fn page(output: &Output) -> String {
    let generator = format!("mcp-trace-validator {}", env!("CARGO_PKG_VERSION"));
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .replace(&generator, "mcp-trace-validator (version)")
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(root().join(format!("corpus/golden/html/{name}.html"))).unwrap()
}

#[test]
fn a_passing_trace_writes_its_golden_page_and_exits_0() {
    let output = run(&[
        "validate",
        "corpus/good/http-session.jsonl",
        "--format",
        "html",
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(page(&output), golden("http-session"));
}

#[test]
fn a_failing_trace_writes_its_golden_page_and_exits_1() {
    let output = run(&[
        "validate",
        "corpus/violations/base-001-request-id-boolean.jsonl",
        "--format",
        "html",
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(page(&output), golden("base-001-request-id-boolean"));
}

#[test]
fn two_requested_revisions_write_one_page_with_both() {
    let output = run(&[
        "validate",
        "corpus/good/stdio-full-session.jsonl",
        "--revision",
        "2025-11-25",
        "--revision",
        "2026-07-28",
        "--format",
        "html",
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(page(&output), golden("stdio-full-session-two-revisions"));
}

#[test]
fn warnings_fail_under_strict_and_the_page_still_says_what_the_trace_did() {
    let trace = "corpus/violations/tool-010-structured-without-text.jsonl";
    let lenient = run(&["validate", trace, "--format", "html"]);
    assert_eq!(lenient.status.code(), Some(0), "{lenient:?}");
    let strict = run(&["validate", trace, "--format", "html", "--strict"]);
    assert_eq!(strict.status.code(), Some(1), "{strict:?}");
    assert_eq!(page(&strict), page(&lenient));
    assert!(String::from_utf8_lossy(&strict.stderr).contains("--strict"));
}

#[test]
fn a_run_lists_the_trace_it_could_not_judge_and_exits_3() {
    let missing = std::env::temp_dir().join(format!("mcp-tv-html-{}.jsonl", std::process::id()));
    let broken = missing.to_str().unwrap();
    std::fs::write(&missing, "not json\n").unwrap();
    let output = run(&[
        "validate",
        "corpus/good/stdio-minimal-init.jsonl",
        broken,
        "--format",
        "html",
    ]);
    std::fs::remove_file(&missing).ok();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let page = page(&output);
    assert!(page.contains("<h1>2 traces</h1>"));
    assert!(page.contains("<span class=\"badge v-incomplete\">Incomplete</span>"));
    assert!(page.contains(&format!("<span class=\"unjudged-name\">{broken}</span>")));
}

#[test]
fn stdin_is_named_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(["validate", "-", "--format", "html"])
        .current_dir(root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let trace = std::fs::read(root().join("corpus/good/stdio-minimal-init.jsonl")).unwrap();
    child.stdin.take().unwrap().write_all(&trace).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(page(&output).contains("<h1 class=\"trace-name\">standard input</h1>"));
}

/// A scratch directory of this test's own.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp-tv-html-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn html_report_writes_the_page_beside_what_the_format_prints() {
    let dir = scratch("beside");
    let file = dir.join("page.html");
    let trace = "corpus/violations/base-001-request-id-boolean.jsonl";
    let output = run(&["validate", trace, "--html-report", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("verdict: fail") && !text.contains("<!DOCTYPE html>"),
        "stdout still carries the human report: {text}"
    );
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.starts_with("<!DOCTYPE html>"), "{written}");
    // The page is the one `--format html` prints for the same run.
    let printed = run(&["validate", trace, "--format", "html"]);
    assert_eq!(written, String::from_utf8(printed.stdout).unwrap());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_html_report_that_cannot_be_written_exits_2_and_says_why() {
    let dir = scratch("unwritable");
    let file = dir.join("no-such-directory").join("page.html");
    let output = run(&[
        "validate",
        "corpus/good/http-session.jsonl",
        "--html-report",
        file.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(&format!(
            "error: cannot write the HTML report to {}: ",
            file.display()
        )),
        "{stderr}"
    );
    assert!(!file.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_help_lists_the_format() {
    let output = run(&["validate", "--help"]);
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("html"), "{help}");
    assert!(help.contains("self-contained HTML page"), "{help}");
}
