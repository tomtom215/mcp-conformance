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

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
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
fn a_trace_that_judges_nothing_is_refused_rather_than_passed() {
    // The engine's answer for an empty trace is honest and useless: no
    // findings, so `verdict: pass`, so exit 0 — and the likeliest cause is that
    // the capture failed. A CI job cannot tell that from a conforming session,
    // which is the silent-green failure this project has already been bitten by
    // once, in its own tap.
    for (tag, contents) in [
        ("empty", ""),
        // Not empty, and still judges nothing: a transport that opened and
        // closed carrying no messages.
        (
            "lifecycle-only",
            concat!(
                r#"{"seq":0,"direction":"client-to-server","transport":"stdio","#,
                r#""kind":"lifecycle","event":"transport-open"}"#,
                "\n"
            ),
        ),
    ] {
        let path = write_temp(tag, contents);
        let output = run(&["validate", path.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(2), "{tag}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("judged no requirement at all"),
            "{tag}: {stderr}"
        );
        // And the same refusal in multi-revision mode, which reaches the
        // engine by a different path.
        let output = run(&[
            "validate",
            "--revision",
            "2025-11-25",
            path.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(2), "{tag} (multi): {output:?}");
        std::fs::remove_file(&path).ok();
    }
}

#[test]
fn violating_trace_exits_one_and_names_the_requirement() {
    // This trace never reaches `initialize`, so it declares no revision; the
    // clause it breaks is a `2025-11-25` one, so the test names that revision.
    let output = run(&[
        "validate",
        "--revision",
        "2025-11-25",
        corpus("violations/life-001-first-message-not-initialize.jsonl")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("revision 2025-11-25 (requested)"), "{text}");
    assert!(text.contains("FAIL  LIFE-001"), "{text}");
    assert!(text.contains("verdict: fail"), "{text}");
}

#[test]
fn the_revision_defaults_to_what_the_trace_declares() {
    // A conforming session of each revision passes with no flag at all — the
    // failure this replaced judged every 2026-07-28 session against 2025-11-25.
    for (trace, revision) in [
        ("good/stdio-minimal-init.jsonl", "2025-11-25"),
        ("draft/good/stateless-session.jsonl", "2026-07-28"),
        (
            "draft/captured/reference-host-2026-07-28-stdio.jsonl",
            "2026-07-28",
        ),
    ] {
        let output = run(&["validate", corpus(trace).to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(0), "{trace}: {output:?}");
        let text = stdout(&output);
        assert!(
            text.contains(&format!("revision {revision} (declared by the trace)")),
            "{trace}: {text}"
        );
        assert!(stderr(&output).is_empty(), "{trace}: {output:?}");
    }
}

#[test]
fn an_undeclared_trace_is_judged_against_the_newest_revision_and_says_so() {
    let output = run(&[
        "validate",
        corpus("violations/life-001-first-message-not-initialize.jsonl")
            .to_str()
            .unwrap(),
        "--format",
        "json",
    ]);
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(report["revision"], "2026-07-28");
    assert_eq!(report["revision_source"], "default");
    let note = stderr(&output);
    assert!(
        note.contains("declares no protocol revision") && note.contains("--revision"),
        "{note}"
    );
}

#[test]
fn a_trace_of_an_unsupported_revision_is_refused_not_misjudged() {
    let path = write_temp(
        "future",
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2027-03-01"}}}}"#,
    );
    let output = run(&["validate", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(stdout(&output).is_empty(), "{output:?}");
    let message = stderr(&output);
    assert!(message.contains("2027-03-01"), "{message}");
    assert!(message.contains("hint: pass --revision"), "{message}");
}

#[test]
fn warnings_pass_by_default_and_fail_under_strict() {
    let trace = corpus("violations/life-004-client-request-before-init-response.jsonl");
    let lenient = run(&["validate", trace.to_str().unwrap()]);
    assert_eq!(lenient.status.code(), Some(0), "{lenient:?}");
    assert!(stdout(&lenient).contains("verdict: pass-with-warnings"));

    let strict = run(&["validate", trace.to_str().unwrap(), "--strict"]);
    assert_eq!(strict.status.code(), Some(1), "{strict:?}");

    // The report's verdict line is a property of the trace, so `--strict` does
    // not rewrite it — which left the run ending `verdict: pass-with-warnings`
    // and exiting 1, with nothing connecting the two. stdout stays the report;
    // the sentence that reconciles them goes to stderr.
    assert_eq!(
        stdout(&strict),
        stdout(&lenient),
        "stdout must not depend on --strict"
    );
    let note = String::from_utf8_lossy(&strict.stderr).into_owned();
    assert!(note.contains("--strict"), "{note}");
    assert!(note.contains("exits 1"), "{note}");
    assert!(
        String::from_utf8_lossy(&lenient.stderr).is_empty(),
        "a lenient run says nothing on stderr"
    );

    // A run `--strict` did not promote says nothing either.
    let clean = run(&[
        "validate",
        corpus("good/stdio-full-session.jsonl").to_str().unwrap(),
        "--strict",
    ]);
    assert_eq!(clean.status.code(), Some(0), "{clean:?}");
    assert!(
        String::from_utf8_lossy(&clean.stderr).is_empty(),
        "{clean:?}"
    );
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
    // The newest revision by default.
    let newest = run(&["requirements", "--format", "json"]);
    assert_eq!(newest.status.code(), Some(0));
    let registry: serde_json::Value = serde_json::from_str(&stdout(&newest)).unwrap();
    assert_eq!(registry["revision"], "2026-07-28");
    assert_eq!(
        run(&["requirements", "--revision", "2024-01-01"])
            .status
            .code(),
        Some(2)
    );

    let human = run(&["requirements", "--revision", "2025-11-25"]);
    assert_eq!(human.status.code(), Some(0));
    let text = stdout(&human);
    assert!(text.contains("LIFE-001"), "{text}");
    assert!(
        text.contains("checks: lifecycle.first-interaction-initialize"),
        "{text}"
    );
    assert!(text.contains("excluded"), "{text}");

    let json = run(&[
        "requirements",
        "--revision",
        "2025-11-25",
        "--format",
        "json",
    ]);
    assert_eq!(json.status.code(), Some(0));
    let registry: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(registry["revision"], "2025-11-25");
}

/// Writes `content` to a unique temp file and returns its path; the caller removes it.
fn write_temp(tag: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("mcp-tv-cli-{tag}-{}.json", std::process::id()));
    std::fs::write(&path, content).unwrap();
    path
}

/// A two-revision set: BASE-001 present throughout, LIFE-009 removed at 2026-07-28,
/// DISC-001 introduced at 2026-07-28. All use a real check, so a good trace passes.
const TWO_REVISION_SET: &str = r#"{
    "revisions": ["2025-11-25", "2026-07-28"],
    "requirements": [
        {"id": "BASE-001", "level": "MUST", "actor": "both",
         "source": {"section": "b#x", "quote": "MUST jsonrpc 2.0"},
         "checks": ["base.jsonrpc-version"]},
        {"id": "LIFE-009", "level": "MUST", "actor": "server",
         "applies": {"removed": "2026-07-28"},
         "source": {"section": "l#y", "quote": "MUST jsonrpc 2.0"},
         "checks": ["base.jsonrpc-version"]},
        {"id": "DISC-001", "level": "MUST", "actor": "server",
         "applies": {"introduced": "2026-07-28"},
         "source": {"section": "d#z", "quote": "MUST jsonrpc 2.0"},
         "checks": ["base.jsonrpc-version"]}
    ]
}"#;

#[test]
fn one_requested_revision_gives_the_full_single_revision_report() {
    let output = run(&[
        "validate",
        corpus("good/stdio-minimal-init.jsonl").to_str().unwrap(),
        "--revision",
        "2025-11-25",
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = stdout(&output);
    assert!(
        text.contains("MCP trace validation — revision 2025-11-25 (requested)"),
        "{text}"
    );
    assert!(text.contains("verdict: pass"), "{text}");
}

#[test]
fn two_requested_revisions_judge_against_both_builtin_registries() {
    let output = run(&[
        "validate",
        corpus("good/stdio-minimal-init.jsonl").to_str().unwrap(),
        "--revision",
        "2025-11-25",
        "--revision",
        "2026-07-28",
    ]);
    let text = stdout(&output);
    assert!(
        text.contains("MCP multi-revision validation — revisions 2025-11-25, 2026-07-28"),
        "{text}"
    );
    assert!(text.contains("2025-11-25:"), "{text}");
}

#[test]
fn multi_revision_json_shows_per_clause_applicability_across_revisions() {
    let set = write_temp("set", TWO_REVISION_SET);
    let output = run(&[
        "validate",
        corpus("good/stdio-minimal-init.jsonl").to_str().unwrap(),
        "--registry-set",
        set.to_str().unwrap(),
        "--revision",
        "2025-11-25",
        "--revision",
        "2026-07-28",
        "--format",
        "json",
    ]);
    std::fs::remove_file(&set).ok();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        report["revisions"],
        serde_json::json!(["2025-11-25", "2026-07-28"])
    );

    let row = |id: &str| {
        report["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap_or_else(|| panic!("row {id} present"))
            .clone()
    };
    // Removed at the boundary: present then absent (null).
    assert_eq!(
        row("LIFE-009")["outcomes"],
        serde_json::json!(["pass", null])
    );
    // Introduced at the boundary: absent then present.
    assert_eq!(
        row("DISC-001")["outcomes"],
        serde_json::json!([null, "pass"])
    );
}

#[test]
fn multi_revision_flag_misuse_and_unknown_revisions_exit_two() {
    let good = corpus("good/stdio-minimal-init.jsonl");
    let good = good.to_str().unwrap();

    // --registry-set without --revision selects from the custom set, like the
    // built-in one: this trace declares 2025-11-25, which the set describes.
    let set = write_temp("set-no-rev", TWO_REVISION_SET);
    let selected = run(&["validate", good, "--registry-set", set.to_str().unwrap()]);
    std::fs::remove_file(&set).ok();
    assert_eq!(selected.status.code(), Some(0), "{selected:?}");
    assert!(
        stdout(&selected).contains("revision 2025-11-25 (declared by the trace)"),
        "{selected:?}"
    );

    // --registry (single-revision) with --revision (multi) is contradictory.
    let mixed = run(&[
        "validate",
        good,
        "--registry",
        good,
        "--revision",
        "2025-11-25",
    ]);
    assert_eq!(mixed.status.code(), Some(2), "{mixed:?}");

    // A revision the built-in set does not describe.
    let unknown = run(&["validate", good, "--revision", "2024-01-01"]);
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    let stderr = String::from_utf8_lossy(&unknown.stderr).into_owned();
    assert!(stderr.contains("does not describe revision"), "{stderr}");

    // One requested revision is a single-revision run, so JUnit works.
    let junit = run(&[
        "validate",
        good,
        "--revision",
        "2025-11-25",
        "--format",
        "junit",
    ]);
    assert_eq!(junit.status.code(), Some(0), "{junit:?}");
    assert!(stdout(&junit).starts_with("<?xml"), "{junit:?}");
}

#[test]
fn junit_format_emits_xml_for_validate_and_rejects_requirements() {
    let output = run(&[
        "validate",
        corpus("violations/life-001-first-message-not-initialize.jsonl")
            .to_str()
            .unwrap(),
        "--format",
        "junit",
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let xml = stdout(&output);
    assert!(xml.starts_with("<?xml version=\"1.0\""), "{xml}");
    assert!(xml.contains("<failure message="), "{xml}");

    let rejected = run(&["requirements", "--format", "junit"]);
    assert_eq!(rejected.status.code(), Some(2), "{rejected:?}");
}

#[test]
fn multi_revision_reports_carry_each_findings_seq_and_reason() {
    let trace = corpus("draft/violations/mrtr-019-retry-reuses-id.jsonl");
    let trace = trace.to_str().unwrap();
    let both = ["--revision", "2025-11-25", "--revision", "2026-07-28"];

    let human = run(&[&["validate", trace], &both[..]].concat());
    assert_eq!(human.status.code(), Some(1), "{human:?}");
    let text = stdout(&human);
    assert!(
        text.contains("2026-07-28 seq 2: the retry reuses id 1"),
        "{text}"
    );
    assert!(text.contains("(requested)"), "{text}");
    // Without --quiet every clause is listed, including those needing nothing.
    assert!(text.contains("=excluded"), "{text}");

    let json = run(&[&["validate", trace, "--format", "json"], &both[..]].concat());
    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(report["revision_source"], "requested");
    assert_eq!(report["verdict"], "fail", "the worst across revisions");
    let row = report["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "MRTR-019")
        .unwrap();
    assert_eq!(row["findings"][0], serde_json::json!([]));
    assert_eq!(row["findings"][1][0]["seq"], 2);
    assert_eq!(row["findings"][1][0]["check"], "mrtr.retry-id-differs");
    // The clause is per revision, like the finding it explains: absent where the
    // clause is, and linked to the judged revision's page where it failed.
    assert_eq!(row["sources"][0], serde_json::Value::Null);
    assert_eq!(
        row["sources"][1]["url"],
        "https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr\
         #client-requirements-basic-workflow"
    );
    assert!(
        text.contains(
            "        spec: \"The JSON-RPC `id` MUST be different between the initial request \
             and the retry, as they are independent requests.\"\n"
        ),
        "{text}"
    );

    let junit = run(&[&["validate", trace, "--format", "junit"], &both[..]].concat());
    assert_eq!(junit.status.code(), Some(1), "{junit:?}");
    let xml = stdout(&junit);
    assert_eq!(xml.matches("<testsuite ").count(), 2, "{xml}");
    assert!(xml.contains("mcp-trace-validator (2025-11-25)"), "{xml}");
    assert!(xml.contains("mcp-trace-validator (2026-07-28)"), "{xml}");
    assert!(xml.contains("[mrtr.retry-id-differs] at seq 2"), "{xml}");
}

#[test]
fn quiet_lists_only_what_needs_attention_and_keeps_every_total() {
    let trace = corpus("draft/violations/mrtr-019-retry-reuses-id.jsonl");
    let full = run(&["validate", trace.to_str().unwrap()]);
    let quiet = run(&["validate", "--quiet", trace.to_str().unwrap()]);
    assert_eq!(quiet.status.code(), full.status.code());
    let (full, quiet) = (stdout(&full), stdout(&quiet));
    assert!(full.contains("  EXCL  "), "{full}");
    assert!(
        !quiet.contains("  EXCL  ") && !quiet.contains("  PASS  "),
        "{quiet}"
    );
    assert!(quiet.contains("FAIL  MRTR-019"), "{quiet}");
    let totals = |text: &str| {
        text.lines()
            .find(|line| line.starts_with("totals:"))
            .map(str::to_owned)
    };
    assert_eq!(
        totals(&quiet),
        totals(&full),
        "--quiet hides rows, never counts"
    );
}

#[test]
fn the_readme_quickstart_validate_command_runs() {
    let readme =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md")).unwrap();
    let start = readme
        .find("## Quickstart")
        .expect("README has a quickstart");
    let end = readme[start..]
        .find("\n## ")
        .map_or(readme.len(), |at| start + at);
    let line = readme[start..end]
        .lines()
        .find(|line| line.starts_with("mcp-trace-validator "))
        .expect("the quickstart shows a validate command");
    // The README's arguments, with its placeholder trace replaced by a committed one.
    let args: Vec<String> = line
        .split_whitespace()
        .skip(1)
        .map(|arg| {
            if arg == "session.jsonl" {
                corpus("draft/violations/mrtr-019-retry-reuses-id.jsonl")
                    .to_str()
                    .unwrap()
                    .to_owned()
            } else {
                arg.to_owned()
            }
        })
        .collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run(&args);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stdout(&output).contains("FAIL  MRTR-019"), "{output:?}");
}

#[test]
fn a_custom_registry_cannot_be_combined_with_revision_selection() {
    // A valid registry file, so the only thing wrong is the combination.
    let registry = write_temp(
        "one-registry",
        r#"{"revision":"2025-11-25","requirements":[
            {"id":"BASE-001","level":"MUST","actor":"both",
             "source":{"section":"b#x","quote":"MUST jsonrpc 2.0"},
             "checks":["base.jsonrpc-version"]}]}"#,
    );
    let set = write_temp("one-set", TWO_REVISION_SET);
    let good = corpus("good/stdio-minimal-init.jsonl");
    let alone = run(&[
        "validate",
        good.to_str().unwrap(),
        "--registry",
        registry.to_str().unwrap(),
    ]);
    for extra in [
        vec!["--revision", "2025-11-25"],
        vec!["--registry-set", set.to_str().unwrap()],
    ] {
        let mut args = vec![
            "validate",
            good.to_str().unwrap(),
            "--registry",
            registry.to_str().unwrap(),
        ];
        args.extend(extra);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("cannot be combined"),
            "{output:?}"
        );
    }
    std::fs::remove_file(&registry).ok();
    std::fs::remove_file(&set).ok();
    assert_eq!(
        alone.status.code(),
        Some(0),
        "the registry alone is fine: {alone:?}"
    );
}

#[test]
fn a_reader_that_stops_early_is_not_an_error() {
    use std::io::BufRead as _;
    // The JSON registry is ~150 KB, well past a pipe's buffer, so the write is
    // certain to meet the closed pipe rather than finish into the buffer first.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(["requirements", "--format", "json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    // The reader is gone; the rest of the output meets a closed pipe.
    let output = child.wait_with_output().unwrap();
    assert!(first.starts_with('{'), "{first}");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_write_error_other_than_a_closed_pipe_is_reported() {
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .arg("requirements")
        .stdout(full)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot write output"),
        "{output:?}"
    );
}

#[test]
fn the_reader_limits_are_flags_and_a_trace_over_one_is_told_which() {
    let trace = corpus("draft/violations/mrtr-019-retry-reuses-id.jsonl");
    let trace = trace.to_str().unwrap();
    let text = std::fs::read_to_string(trace).unwrap();
    let longest = text.lines().map(str::len).max().unwrap();
    let events = text.lines().count();

    let short = longest - 1;
    let output = run(&["validate", trace, "--max-line-bytes", &short.to_string()]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        stderr(&output).contains(&format!(
            "hint: if the recording is sound, re-run with --max-line-bytes {longest} or more"
        )),
        "{}",
        stderr(&output)
    );
    let exact = run(&["validate", trace, "--max-line-bytes", &longest.to_string()]);
    assert_eq!(exact.status.code(), Some(1), "{exact:?}");

    let few = (events - 1).to_string();
    let output = run(&["validate", trace, "--max-events", &few]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        stderr(&output).contains(&format!("re-run with --max-events above {few}")),
        "{}",
        stderr(&output)
    );
    let all = run(&["validate", trace, "--max-events", &events.to_string()]);
    assert_eq!(all.status.code(), Some(1), "{all:?}");

    // Any other malformation gets no limit hint.
    let blank = run(&["validate", corpus("README.md").to_str().unwrap()]);
    assert_eq!(blank.status.code(), Some(3), "{blank:?}");
    assert!(!stderr(&blank).contains("hint:"), "{}", stderr(&blank));
}

#[test]
fn sarif_is_a_validate_format_with_the_same_exit_codes() {
    let trace = corpus("draft/violations/mrtr-019-retry-reuses-id.jsonl");
    let trace = trace.to_str().unwrap();
    let output = run(&["validate", trace, "--format", "sarif"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let log: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(log["version"], "2.1.0");
    let result = &log["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "MRTR-019");
    // The trace was named by an absolute path, so the location is a file: URI
    // (a drive-letter one on Windows).
    let uri = result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
        .as_str()
        .unwrap();
    assert!(uri.starts_with("file:///"), "{uri}");
    assert!(
        uri.ends_with("/corpus/draft/violations/mrtr-019-retry-reuses-id.jsonl"),
        "{uri}"
    );
    assert_eq!(
        result["locations"][0]["physicalLocation"]["region"]["startLine"],
        3
    );
    // Several revisions give one run with each revision's findings.
    let both = run(&[
        "validate",
        trace,
        "--format",
        "sarif",
        "--revision",
        "2025-11-25",
        "--revision",
        "2026-07-28",
    ]);
    assert_eq!(both.status.code(), Some(1), "{both:?}");
    let log: serde_json::Value = serde_json::from_str(&stdout(&both)).unwrap();
    assert_eq!(
        log["runs"][0]["properties"]["revisions"],
        serde_json::json!(["2025-11-25", "2026-07-28"])
    );
    let rejected = run(&["requirements", "--format", "sarif"]);
    assert_eq!(rejected.status.code(), Some(2), "{rejected:?}");
}

#[test]
fn quiet_multi_revision_output_hides_rows_never_counts() {
    let trace = corpus("draft/violations/mrtr-019-retry-reuses-id.jsonl");
    let trace = trace.to_str().unwrap();
    let both = ["--revision", "2025-11-25", "--revision", "2026-07-28"];
    let text = stdout(&run(&[&["validate", trace], &both[..]].concat()));
    let quiet = run(&[&["validate", "--quiet", trace], &both[..]].concat());
    assert_eq!(quiet.status.code(), Some(1), "{quiet:?}");
    let quiet = stdout(&quiet);
    assert!(quiet.contains("MRTR-019"), "{quiet}");
    assert!(!quiet.contains("=excluded"), "{quiet}");
    let per_revision = |text: &str| {
        text.lines()
            .skip_while(|line| *line != "per revision:")
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        per_revision(&quiet),
        per_revision(&text),
        "--quiet hides rows, never counts"
    );
}
