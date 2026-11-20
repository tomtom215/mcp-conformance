// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `check`, end to end: the real binary against this workspace's everything
//! server over HTTP at both revisions (in process, on loopback), and against
//! `sh` stand-ins over stdio for the paths a conforming server never takes — a
//! server the probe kills, one that negotiates a revision nobody judges here,
//! one that never answers, one that asks the client for something.
//!
//! The stdio leg against the real everything server binary is `cargo xtask
//! conformance`'s: a test of this crate cannot name another package's binary.

// The helpers `check_edges` shares are `pub(crate)`, which rustc's
// `unreachable_pub` asks for and clippy's `redundant_pub_crate` objects to;
// this follows rustc, as the crates' private modules do.
#![allow(clippy::redundant_pub_crate)]

use std::net::SocketAddr;

use mcp_everything_server::policy::HttpSecurityPolicy;
use mcp_everything_server::server::ServedRevision;
use serde_json::Value;

use super::*;

/// The everything server on a loopback port, for the life of `runtime`.
pub(crate) fn serve(runtime: &tokio::runtime::Runtime, revision: ServedRevision) -> SocketAddr {
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = mcp_everything_server::http::router(HttpSecurityPolicy::default(), revision);
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        address
    })
}

pub(crate) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

pub(crate) fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The exit status of `validate` over `trace` with no party view: both ends
/// of the session held to their clauses, the exerciser included.
fn judged_for_both_parties(trace: &Path) -> Output {
    binary()
        .args(["validate", "--strict"])
        .arg(trace)
        .output()
        .unwrap()
}

#[test]
fn a_modern_http_server_is_exercised_and_the_exerciser_itself_breaks_no_clause() {
    let runtime = runtime();
    let address = serve(&runtime, ServedRevision::V2026_07_28);
    let dir = scratch("check-http-modern");
    let (trace, summary) = (dir.join("t.jsonl"), dir.join("s.json"));
    let output = binary()
        .args(["check", "--call-all", "--url"])
        .arg(format!("http://{address}/mcp"))
        .arg("-o")
        .arg(&trace)
        .arg("--summary")
        .arg(&summary)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("verdict (server findings only): pass"),
        "{}",
        stdout(&output)
    );
    let summary = json(&summary);
    assert_eq!(summary["revision"], "2026-07-28");
    assert_eq!(summary["transport"], "streamable-http");
    assert_eq!(
        summary["server"],
        format!("mcp-everything-server {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(summary["tools"]["listed"], summary["tools"]["exercised"]);
    assert!(
        summary["tools"]["listed"].as_u64().unwrap() >= 10,
        "{summary}"
    );
    assert!(
        summary["resources"]["exercised"].as_u64().unwrap() >= 1,
        "{summary}"
    );
    assert_eq!(summary["trace"], trace.display().to_string());
    prompts_and_errors_are_accounted_for(&summary);
    // The recording shows the transport's request metadata, a designated
    // parameter mirrored among it, and no legacy session.
    let recorded = std::fs::read_to_string(&trace).unwrap();
    for needle in [
        "\"mcp-method\":\"tools/call\"",
        "\"mcp-name\":",
        "\"mcp-param-",
    ] {
        assert!(recorded.contains(needle), "{needle}");
    }
    assert!(
        !recorded.contains("\"initialize\""),
        "no handshake at 2026-07-28"
    );
    // Strict, and with no party view: a slip of the exerciser's own would
    // fail here.
    let both = judged_for_both_parties(&trace);
    assert_eq!(both.status.code(), Some(0), "{}", stdout(&both));
}

/// What the summary of a `--call-all` run against the everything server
/// says of its prompts and of the tools that answer with an error.
fn prompts_and_errors_are_accounted_for(summary: &Value) {
    // The everything server lists five prompts, two of which require
    // arguments: those two are named and left, the other three fetched.
    assert_eq!(summary["prompts"]["exercised"], 3, "{summary}");
    let mut left = names(summary, "requires arguments");
    left.sort();
    assert_eq!(
        left,
        [
            "test_prompt_with_arguments",
            "test_prompt_with_embedded_resource"
        ]
    );
    // Some of its tools need a client capability this client does not
    // declare, and answer with a JSON-RPC error: counted, as errors.
    let errors = summary["tools"]["errors"].as_u64().unwrap();
    assert!(
        errors >= 1 && errors < summary["tools"]["exercised"].as_u64().unwrap(),
        "{summary}"
    );
}

#[test]
fn a_legacy_http_session_is_opened_with_initialize_and_ended_with_delete() {
    let runtime = runtime();
    let address = serve(&runtime, ServedRevision::V2025_11_25);
    let dir = scratch("check-http-legacy");
    let (trace, summary) = (dir.join("t.jsonl"), dir.join("s.json"));
    let output = binary()
        .args(["check", "--spec-version", "2025-11-25", "--url"])
        .arg(format!("http://{address}/mcp"))
        .args(["--header", "Authorization: Bearer s3cret-token", "-o"])
        .arg(&trace)
        .arg("--summary")
        .arg(&summary)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let summary = json(&summary);
    assert_eq!(summary["revision"], "2025-11-25");
    assert_eq!(summary["unanswered"], serde_json::json!([]));
    // Unannotated tools are named, not called.
    assert!(summary["tools"]["exercised"].as_u64() < summary["tools"]["listed"].as_u64());
    assert!(
        summary["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|one| one["kind"] == "tool" && one["reason"].as_str().unwrap().contains("--call")),
        "{summary}"
    );
    let recorded = std::fs::read_to_string(&trace).unwrap();
    assert!(
        recorded.contains("\"mcp-session-id\""),
        "the session is carried"
    );
    assert!(recorded.contains("\"method\":\"DELETE\""), "and ended");
    assert!(
        !recorded.contains("server/discover"),
        "no probe when the revision is named"
    );
    // The credential went to the server and nowhere else.
    assert!(!recorded.contains("s3cret-token"));
    assert!(!stderr(&output).contains("s3cret-token"));
    let both = judged_for_both_parties(&trace);
    assert_eq!(both.status.code(), Some(0), "{}", stdout(&both));
}

#[test]
fn the_report_takes_validates_flags_and_formats() {
    let runtime = runtime();
    let address = serve(&runtime, ServedRevision::V2026_07_28);
    let output = binary()
        .args(["check", "--format", "json", "--url"])
        .arg(format!("http://{address}/mcp"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    // Stdout is the report alone: the run's account went to stderr.
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["revision"], "2026-07-28");
    assert!(stderr(&output).contains("mcp-conformance check: http://"));
}

/// A stdio server in `sh`: reads requests line by line and answers by method,
/// echoing each request's id. `$1` selects the behaviour under test.
pub(crate) const SERVER: &str = r#"mode=$1
answer() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$id" "$1"; }
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"server/discover"'*)
      [ "$mode" = dies-on-probe ] && exit 3
      [ "$mode" = modern-annotated ] && answer '{"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}' && continue
      printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$id" ;;
    *'"initialize"'*)
      [ "$mode" = silent ] && continue
      [ "$mode" = lingers ] && printf '%s' "$$" > "$2"
      [ "$mode" = unterminated ] && printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"sh-server","version":"1"}}}' "$id" && exit 0
      version=2025-11-25
      [ "$mode" = old ] && version=2024-11-05
      answer '{"protocolVersion":"'$version'","capabilities":{"tools":{}},"serverInfo":{"name":"sh-server","version":"1"}}' ;;
    *'"ping"'*)
      [ "$mode" = asks ] && printf '{"jsonrpc":"2.0","id":"s-1","method":"roots/list"}\n' && IFS= read -r reply && printf '%s\n' "$reply" > "$2"
      answer '{}'
      [ "$mode" = dies-later ] && exit 0 ;;
    *'"tools/list"'*)
      [ "$mode" = annotated ] || [ "$mode" = modern-annotated ] && answer '{"tools":[{"name":"mirrors","inputSchema":{"type":"object","properties":{"tags":{"type":"array","items":{"type":"string","x-mcp-header":"Tag"}}}},"annotations":{"readOnlyHint":true}}]}' && continue
      answer '{"tools":[{"name":"look","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},{"name":"touch","inputSchema":{"type":"object","required":["n"],"properties":{"n":{"type":"integer","minimum":3}}}},{"name":"odd","inputSchema":{"type":"object","required":["p"],"properties":{"p":{"type":"string","pattern":"^a+$"}}},"annotations":{"readOnlyHint":true}}]}' ;;
    *'"tools/call"'*)
      printf '%s\n' "$line" >> "$2"
      [ "$mode" = wants-input ] && answer '{"resultType":"input_required","inputRequests":{}}' && continue
      answer '{"content":[{"type":"text","text":"done"}]}' ;;
  esac
done
[ "$mode" = lingers ] && sleep 10
"#;

/// `check` over the stand-in in `mode`; returns the output, the run's
/// summary, and what the stand-in noted in its side file.
pub(crate) fn stand_in(name: &str, mode: &str, flags: &[&str]) -> (Output, Value, String) {
    let dir = scratch(name);
    let (script, noted, summary) = (dir.join("server.sh"), dir.join("noted"), dir.join("s.json"));
    std::fs::write(&script, SERVER).unwrap();
    let mut command = binary();
    command.arg("check").args(flags);
    // Two seconds an answer unless the test says otherwise: the stand-in
    // answers at once, and a client that has stopped hearing it (which is
    // what several mutations of the transport produce) should fail a test
    // in seconds, not hang it for the default thirty.
    if !flags.contains(&"--timeout") {
        command.args(["--timeout", "2"]);
    }
    command
        .arg("--summary")
        .arg(&summary)
        .arg("-o")
        .arg(dir.join("t.jsonl"))
        .args(["--", "sh"])
        .arg(&script)
        .arg(mode)
        .arg(&noted);
    // A stand-in that outlives `check` must not hold the test: it inherits
    // these, and a pipe would stay open as long as it lives.
    let (out, err) = (dir.join("stdout"), dir.join("stderr"));
    let status = command
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap())
        .status()
        .unwrap();
    let output = Output {
        status,
        stdout: std::fs::read(&out).unwrap(),
        stderr: std::fs::read(&err).unwrap(),
    };
    let summary = std::fs::read_to_string(&summary)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    (
        output,
        summary,
        std::fs::read_to_string(&noted).unwrap_or_default(),
    )
}

pub(crate) fn names(summary: &Value, reason: &str) -> Vec<String> {
    summary["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|one| one["reason"].as_str().unwrap().contains(reason))
        .map(|one| one["name"].as_str().unwrap().to_owned())
        .collect()
}

#[cfg(unix)]
#[test]
fn only_read_only_tools_are_called_and_every_other_is_named_with_its_reason() {
    let (output, summary, calls) = stand_in("check-default", "plain", &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(summary["revision"], "2025-11-25");
    assert!(
        summary["negotiation"]
            .as_str()
            .unwrap()
            .contains("fell back"),
        "{summary}"
    );
    assert_eq!(summary["tools"]["listed"], 3);
    assert_eq!(summary["tools"]["exercised"], 1);
    assert_eq!(names(&summary, "not annotated read-only"), ["touch"]);
    assert_eq!(names(&summary, "no arguments were built"), ["odd"]);
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(
        calls.contains(r#""name":"look""#) && calls.contains(r#""arguments":{}"#),
        "{calls}"
    );
}

#[cfg(unix)]
#[test]
fn a_named_tool_is_called_with_arguments_built_from_its_schema() {
    let flags = [
        "--spec-version",
        "2025-11-25",
        "--call",
        "touch",
        "--call",
        "ghost",
    ];
    let (output, summary, calls) = stand_in("check-call", "plain", &flags);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(summary["tools"]["exercised"], 2);
    assert!(calls.contains(r#""arguments":{"n":3}"#), "{calls}");
    assert_eq!(names(&summary, "does not list it"), ["ghost"]);
    assert!(
        !calls.contains("ghost"),
        "an unlisted name is said, never sent"
    );
    // One name alone: only that tool is added to the read-only one.
    let (_, summary, calls) = stand_in("check-call-one", "plain", &["--call", "touch"]);
    assert_eq!(summary["tools"]["exercised"], 2);
    assert_eq!(calls.lines().count(), 2, "{calls}");
    // `--call-all` does not call what no arguments can be built for.
    let (_, summary, calls) = stand_in("check-call-all", "plain", &["--call-all"]);
    assert_eq!(summary["tools"]["exercised"], 2);
    assert!(!calls.contains(r#""name":"odd""#), "{calls}");
}

#[cfg(unix)]
#[test]
fn the_budgets_stop_the_run_and_say_which_did() {
    let flags = [
        "--spec-version",
        "2025-11-25",
        "--call-all",
        "--max-tool-calls",
        "1",
    ];
    let (_, summary, calls) = stand_in("check-max-calls", "plain", &flags);
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert_eq!(names(&summary, "--max-tool-calls"), ["touch"]);
    // initialize, ping, then the third request is the last allowed.
    let flags = ["--spec-version", "2025-11-25", "--max-requests", "3"];
    let (output, summary, calls) = stand_in("check-max-requests", "plain", &flags);
    assert_eq!(summary["requests"], 3);
    assert!(
        summary["stopped_by"]
            .as_str()
            .unwrap()
            .contains("--max-requests"),
        "{summary}"
    );
    assert_eq!(calls, "");
    assert!(
        stderr(&output).contains("stopped early"),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn a_server_the_probe_ends_is_started_again_and_opened_with_initialize() {
    let (output, summary, _) = stand_in("check-restart", "dies-on-probe", &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        summary["negotiation"]
            .as_str()
            .unwrap()
            .contains("started again"),
        "{summary}"
    );
    assert_eq!(summary["revision"], "2025-11-25");
    assert_eq!(summary["unanswered"], serde_json::json!([]));
    // The trace is the second process's session alone.
    let trace = std::fs::read_to_string(scratch_path("check-restart").join("t.jsonl")).unwrap();
    assert!(!trace.contains("server/discover"), "{trace}");
}

#[cfg(unix)]
#[test]
fn a_revision_this_build_does_not_judge_ends_the_session_and_the_run() {
    let (output, summary, calls) = stand_in("check-old", "old", &[]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("\"2024-11-05\""),
        "{}",
        stderr(&output)
    );
    assert_eq!(summary["revision"], Value::Null);
    assert_eq!(
        summary["tools"]["declared"], false,
        "nothing is asked of it"
    );
    assert_eq!(calls, "");
    assert_eq!(stdout(&output), "", "no report for a session never held");
}

#[cfg(unix)]
#[test]
fn a_server_that_never_answers_is_a_run_that_could_not_be_made() {
    let flags = ["--spec-version", "2025-11-25", "--timeout", "1"];
    let (output, summary, _) = stand_in("check-silent", "silent", &flags);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no session was established"),
        "{}",
        stderr(&output)
    );
    assert!(
        summary["unanswered"][0]
            .as_str()
            .unwrap()
            .contains("--timeout"),
        "{summary}"
    );
}

#[cfg(unix)]
#[test]
fn a_request_from_the_server_is_refused_and_counted() {
    let flags = ["--spec-version", "2025-11-25"];
    let (output, summary, reply) = stand_in("check-asks", "asks", &flags);
    assert_eq!(summary["server_requests"], 1, "{}", stderr(&output));
    let reply: Value = serde_json::from_str(reply.lines().next().unwrap()).unwrap();
    assert_eq!(reply["id"], "s-1");
    assert_eq!(reply["error"]["code"], -32601);
    assert!(
        stderr(&output).contains("sent this client 1 request(s)"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_server_that_cannot_start_leaves_no_trace_behind() {
    let dir = scratch("check-missing");
    let trace = dir.join("t.jsonl");
    let output = binary()
        .args(["check", "-o"])
        .arg(&trace)
        .args(["--", "/nonexistent/mcp-server"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("cannot start /nonexistent/mcp-server"),
        "{}",
        stderr(&output)
    );
    assert!(!trace.exists());
}

#[test]
fn the_subject_is_one_of_a_url_or_a_command() {
    for arguments in [
        vec!["check"],
        vec!["check", "--url", "http://127.0.0.1:1/mcp", "--", "server"],
        vec![
            "check",
            "--url",
            "http://127.0.0.1:1/mcp",
            "--command",
            "server",
        ],
        vec!["check", "--force", "--", "server"],
    ] {
        let output = binary().args(&arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
    }
    let output = binary()
        .args(["check", "--header", "A: b", "--", "sh", "-c", "exit 0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--header applies to --url"),
        "{}",
        stderr(&output)
    );
    // A URL's query can carry a key; it is neither printed nor echoed back.
    let output = binary()
        .args(["check", "--url", "ftp://host/mcp?key=s3cret"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!stderr(&output).contains("s3cret"), "{}", stderr(&output));
}

#[test]
fn an_unreachable_url_is_a_run_that_could_not_be_made() {
    // A port nothing listens on: bound, read, and released.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let output = binary()
        .args(["check", "--url"])
        .arg(format!("http://127.0.0.1:{port}/mcp?key=s3cret"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no session was established"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("s3cret"), "{}", stderr(&output));
}

#[test]
fn the_validators_subcommands_are_offered_under_this_name() {
    let listing = binary()
        .args(["requirements", "LIFE-001"])
        .output()
        .unwrap();
    assert_eq!(listing.status.code(), Some(0), "{}", stderr(&listing));
    assert!(stdout(&listing).contains("LIFE-001"));
    let judged = binary()
        .args(["validate", "../../corpus/good/stdio-cancellation.jsonl"])
        .output()
        .unwrap();
    assert_eq!(judged.status.code(), Some(0), "{}", stderr(&judged));
    assert!(
        stdout(&judged).contains("verdict: pass"),
        "{}",
        stdout(&judged)
    );
    let script = binary().args(["completions", "bash"]).output().unwrap();
    // Not the binary's name spelled with its hyphen: the oldest clap_complete
    // the manifest allows writes it another way (seen in CI's minimal-versions
    // job). The subcommands are what the script is for.
    let script = stdout(&script);
    assert!(
        script.contains("check") && script.contains("validate"),
        "{script}"
    );
    let version = binary().arg("--version").output().unwrap();
    assert!(
        stdout(&version).contains("judges protocol revisions: "),
        "{}",
        stdout(&version)
    );
}
