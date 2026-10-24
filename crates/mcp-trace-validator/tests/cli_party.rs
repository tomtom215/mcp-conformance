// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Who broke it, and `--party`: a server author running the official suite
//! sees the suite's own client break LIFE-003 and LIFE-009, and needs the
//! report to say so — and, on request, a verdict that counts only the server.

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

/// The two findings the suite's client leaves behind: LIFE-003 (it never sent
/// `notifications/initialized`) and LIFE-009 (it called `logging/setLevel` on
/// a server that declared no `logging`).
fn suite_client_trace(tag: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("mcp-tv-party-{tag}-{}.jsonl", std::process::id()));
    std::fs::write(
        &path,
        concat!(
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"suite","version":"0"}}}}"#,
            "\n",
            r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}}"#,
            "\n",
            r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"logging/setLevel","params":{"level":"info"}}}"#,
            "\n",
            r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"Method not found"}}}"#,
            "\n",
        ),
    )
    .unwrap();
    path
}

#[test]
fn every_finding_names_its_party_and_the_default_verdict_is_unchanged() {
    let path = suite_client_trace("default");
    let trace = path.to_str().unwrap();
    let human = run(&["validate", trace]);
    let json = run(&["validate", "--format", "json", trace]);
    std::fs::remove_file(&path).ok();

    assert_eq!(human.status.code(), Some(1), "{human:?}");
    let text = stdout(&human);
    assert!(
        text.contains(
            "  FAIL  LIFE-003 (MUST)\n        [client] seq 1: the server answered initialize here"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "  FAIL  LIFE-009 (MUST)\n        [client] seq 2: \"logging/setLevel\" uses the \
             server capability logging"
        ),
        "{text}"
    );
    assert!(text.contains("\nverdict: fail\n"), "{text}");

    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert!(report.get("party_view").is_none(), "{report}");
    let rows = report["requirements"].as_array().unwrap();
    let row = |id: &str| rows.iter().find(|row| row["id"] == id).unwrap();
    assert_eq!(row("LIFE-003")["actor"], "client");
    assert_eq!(row("LIFE-003")["findings"][0]["party"], "client");
    assert_eq!(row("LIFE-009")["actor"], "both");
    assert_eq!(row("LIFE-009")["findings"][0]["party"], "client");
    assert!(rows.iter().all(|row| row.get("actor").is_some()));
}

#[test]
fn party_server_counts_only_the_servers_findings_and_says_so() {
    let path = suite_client_trace("view");
    let trace = path.to_str().unwrap();
    let server = run(&["validate", "--party", "server", trace]);
    let client = run(&["validate", "--party", "client", trace]);
    let json = run(&["validate", "--party", "server", "--format", "json", trace]);
    let bad = run(&["validate", "--party", "both", trace]);
    std::fs::remove_file(&path).ok();

    assert_eq!(server.status.code(), Some(0), "{server:?}");
    let text = stdout(&server);
    assert!(
        text.contains("\nverdict (server findings only): pass — 2 client findings set aside\n"),
        "{text}"
    );
    // The rows and totals are still the trace's.
    assert!(text.contains("  FAIL  LIFE-003 (MUST)"), "{text}");
    assert!(text.contains(", 2 fail, "), "{text}");

    assert_eq!(client.status.code(), Some(1), "{client:?}");
    assert!(
        stdout(&client).contains("\nverdict (client findings only): fail\n"),
        "{}",
        stdout(&client)
    );

    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(report["party_view"], "server");
    assert_eq!(report["verdict"], "pass");
    assert_eq!(report["totals"]["fail"], 2);

    assert_eq!(bad.status.code(), Some(2), "{bad:?}");
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("invalid value 'both'"),
        "{bad:?}"
    );
}

#[test]
fn the_machine_formats_carry_the_party_view_too() {
    let path = suite_client_trace("formats");
    let trace = path.to_str().unwrap();
    let junit = run(&["validate", "--party", "server", "--format", "junit", trace]);
    let sarif = run(&["validate", "--party", "server", "--format", "sarif", trace]);
    let html = run(&["validate", "--party", "server", "--format", "html", trace]);
    std::fs::remove_file(&path).ok();

    assert_eq!(junit.status.code(), Some(0), "{junit:?}");
    let xml = stdout(&junit);
    assert!(xml.contains(r#" failures="0""#), "{xml}");
    assert!(
        xml.contains("set aside: this report counts server findings only"),
        "{xml}"
    );

    assert_eq!(sarif.status.code(), Some(0), "{sarif:?}");
    let log: serde_json::Value = serde_json::from_str(&stdout(&sarif)).unwrap();
    assert_eq!(log["runs"][0]["properties"]["party_view"], "server");
    assert!(
        log["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|result| result["level"] == "note" && result["properties"]["counted"] == false),
        "{log}"
    );

    assert_eq!(html.status.code(), Some(0), "{html:?}");
    let page = stdout(&html);
    assert!(
        page.contains("<span class=\"badge v-pass\">Pass</span>"),
        "{page}"
    );
    assert!(
        page.contains("server findings only (2 client findings set aside)"),
        "{page}"
    );
}

#[test]
fn party_applies_to_a_multi_revision_run_and_to_several_traces() {
    let path = suite_client_trace("multi");
    let trace = path.to_str().unwrap();
    let good = corpus("good/stdio-minimal-init.jsonl");
    let multi = run(&[
        "validate",
        "--party",
        "server",
        "--revision",
        "2025-11-25",
        "--revision",
        "2026-07-28",
        trace,
    ]);
    let several = run(&["validate", "--party", "client", trace, &good]);
    std::fs::remove_file(&path).ok();

    // At 2025-11-25 everything held against the server is the client's; at
    // 2026-07-28 the server's own `initialize` result lacks `resultType`
    // (BASE-048, and the wire schema), so the server's view passes the first
    // revision and fails the second — and the run.
    assert_eq!(multi.status.code(), Some(1), "{multi:?}");
    let text = stdout(&multi);
    assert!(
        text.contains(
            "  2025-11-25: 19 pass, 2 fail, 0 warn, 232 excluded, 0 unsupported, 6 not \
             applicable, 62 not observed — verdict pass (server findings only: 2 client \
             findings set aside)\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("— verdict fail (server findings only: 28 client findings set aside)\n"),
        "{text}"
    );
    assert!(
        text.contains("overall verdict (server findings only): fail"),
        "{text}"
    );
    assert!(
        text.contains("[server] 2026-07-28 seq 1: result has no `resultType`"),
        "{text}"
    );

    assert_eq!(several.status.code(), Some(1), "{several:?}");
    let text = stdout(&several);
    assert!(
        text.contains("verdict (client findings only): fail"),
        "{text}"
    );
    assert!(
        text.contains("verdict (client findings only): pass"),
        "{text}"
    );
}
