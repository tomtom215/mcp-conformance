// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The unanswered-requests note: a server that never answers a request, or
//! exits with one pending, used to produce `verdict: pass` and nothing else.
//! The verdict still passes — no clause judges it — but every format now says
//! what went unanswered and how the trace ended.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::{Command, Output};

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

const AT_CLOSE: &str = "good/stdio-request-unanswered-at-close.jsonl";
const PARTLY: &str = "good/stdio-requests-partly-unanswered.jsonl";

const NOTE: &str = "  NOTE  1 request had no response when the trace ended (the transport \
                    closed).\n        JSON-RPC expects every request answered; no clause of this \
                    revision judges it, so the verdict is unchanged.\n        client \
                    \"tools/list\" id 2 at seq 3\n";

#[test]
fn a_request_left_unanswered_at_close_is_noted_and_the_verdict_still_passes() {
    let trace = corpus(AT_CLOSE);
    let human = run(&["validate", &trace]);
    assert_eq!(human.status.code(), Some(0), "{human:?}");
    let text = stdout(&human);
    assert!(text.contains(NOTE), "{text}");
    assert!(text.ends_with("verdict: pass\n"), "{text}");

    let json = run(&["validate", "--format", "json", &trace]);
    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(report["verdict"], "pass");
    assert_eq!(
        report["unanswered_requests"],
        serde_json::json!({
            "count": 1,
            "ending": "transport-close",
            "requests": [{"method": "tools/list", "id": 2, "seq": 3, "sender": "client"}],
        }),
        "{report}"
    );
}

#[test]
fn the_machine_formats_carry_the_note_without_failing_anything() {
    let trace = corpus(AT_CLOSE);
    let junit = run(&["validate", "--format", "junit", &trace]);
    assert_eq!(junit.status.code(), Some(0), "{junit:?}");
    let xml = stdout(&junit);
    assert!(
        xml.contains(
            "    <properties><property name=\"unanswered-requests\" value=\"1 request had no \
             response when the trace ended (the transport closed): client &quot;tools/list&quot; \
             id 2 at seq 3\"/></properties>\n"
        ),
        "{xml}"
    );
    assert!(xml.contains(r#" failures="0""#), "{xml}");

    let sarif = run(&["validate", "--format", "sarif", &trace]);
    assert_eq!(sarif.status.code(), Some(0), "{sarif:?}");
    let log: serde_json::Value = serde_json::from_str(&stdout(&sarif)).unwrap();
    let invocation = &log["runs"][0]["invocations"][0];
    assert_eq!(
        invocation["executionSuccessful"], true,
        "a note is not a failure"
    );
    let note = &invocation["toolExecutionNotifications"][0];
    assert_eq!(note["level"], "note");
    assert_eq!(note["properties"]["kind"], "unanswered-requests");
    assert_eq!(note["properties"]["count"], 1);
    assert_eq!(note["properties"]["ending"], "transport-close");
    assert_eq!(note["properties"]["requests"][0]["seq"], 3);
    assert!(
        note["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap()
            .ends_with("stdio-request-unanswered-at-close.jsonl"),
        "{note}"
    );

    let html = run(&["validate", "--format", "html", &trace]);
    assert_eq!(html.status.code(), Some(0), "{html:?}");
    let page = stdout(&html);
    assert!(
        page.contains(
            "<aside class=\"note\" role=\"note\"><strong>1 request had no response when the trace \
             ended (the transport closed).</strong> JSON-RPC expects every request answered"
        ),
        "{page}"
    );
    assert!(
        page.contains(
            "<li><a href=\"#e3\">seq 3</a> client <code>tools/list</code> id <code>2</code></li>"
        ),
        "{page}"
    );
}

#[test]
fn a_server_that_answered_one_of_three_is_noted_with_the_two_it_left_pending() {
    let human = run(&["validate", &corpus(PARTLY)]);
    assert_eq!(human.status.code(), Some(0), "{human:?}");
    let text = stdout(&human);
    assert!(
        text.contains(
            "  NOTE  2 requests had no response when the trace ended (the recording simply \
             stopped).\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "        client \"ping\" id 3 at seq 5\n        client \"tools/call\" id 4 at seq 6\n"
        ),
        "{text}"
    );
    assert!(
        !text.contains("tools/list\" id 2"),
        "the answered one is not listed: {text}"
    );
    assert!(text.ends_with("verdict: pass\n"), "{text}");

    // A trace whose every request was answered says nothing.
    let full = corpus("good/stdio-full-session.jsonl");
    let quiet = run(&["validate", "--format", "json", &full]);
    let report: serde_json::Value = serde_json::from_str(&stdout(&quiet)).unwrap();
    assert!(report.get("unanswered_requests").is_none(), "{report}");
    assert!(!stdout(&run(&["validate", &full])).contains("NOTE"));
}

#[test]
fn under_two_revisions_the_note_is_written_once() {
    let trace = corpus(PARTLY);
    let both = ["--revision", "2025-11-25", "--revision", "2026-07-28"];
    let multi = run(&[&["validate", &trace], &both[..]].concat());
    let text = stdout(&multi);
    assert_eq!(
        text.matches("  NOTE  2 requests had no response").count(),
        1,
        "{text}"
    );
    assert!(text.contains("at seq 6\nper revision:\n"), "{text}");

    let json = run(&[&["validate", "--format", "json", &trace], &both[..]].concat());
    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(report["unanswered_requests"]["count"], 2, "{report}");
    assert!(report["summaries"][0].get("unanswered_requests").is_none());

    // SARIF notes it once for the trace, not once per revision.
    let sarif = run(&[&["validate", "--format", "sarif", &trace], &both[..]].concat());
    let log: serde_json::Value = serde_json::from_str(&stdout(&sarif)).unwrap();
    let notes = log["runs"][0]["invocations"][0]["toolExecutionNotifications"]
        .as_array()
        .unwrap();
    assert_eq!(notes.len(), 1, "{log}");
}
