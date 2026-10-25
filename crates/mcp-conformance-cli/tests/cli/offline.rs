// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A whole `test --command` run against stand-ins: `node` and `npx` are `sh`
//! scripts, the "suite" drives the bridge with `curl` and writes a
//! `checks.json`, and the "server" is a `sh` loop answering `initialize` and
//! `ping`. Everything real in between — the Node check, the pin check, the
//! bridge, the stdio wrapper, the judging, the report and the exit code — runs
//! as it does against the real suite.

use std::os::unix::fs::PermissionsExt as _;

use super::*;

const NODE: &str = "#!/bin/sh\necho v20.11.0\n";

/// `npx --yes <pkg>@<pin> <args…>`, as the suite would answer them.
const NPX: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo 10.0.0; exit 0; fi
shift 2
case "$1" in
  --version) echo "$FAKE_SUITE_VERSION"; exit 0 ;;
  list)
    printf 'Server scenarios (test against a server):\n  - ping [2025-11-25]\n  - dns-rebinding-protection [2025-11-25]\n  - json-schema-2020-12 [2025-11-25]\n'
    exit 0 ;;
  server) ;;
  *) echo "unexpected: $*" >&2; exit 9 ;;
esac
shift
while [ $# -gt 0 ]; do
  case "$1" in
    --url) url=$2; shift 2 ;;
    --scenario) scenario=$2; shift 2 ;;
    --output-dir) out=$2; shift 2 ;;
    *) shift ;;
  esac
done
headers=$(curl -s -D - -o /dev/null -X POST "$url" -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fake-suite","version":"0"}}}')
session=$(printf '%s\n' "$headers" | tr -d '\r' | grep -i '^mcp-session-id:' | sed 's/^[^:]*: *//')
curl -s -o /dev/null -X POST "$url" -H "mcp-session-id: $session" \
  --data '{"jsonrpc":"2.0","method":"notifications/initialized"}'
curl -s -o /dev/null -X POST "$url" -H "mcp-session-id: $session" \
  --data '{"jsonrpc":"2.0","id":2,"method":"ping"}'
# The suite's own client breaking a clause the server is not party to: it uses
# `logging`, which the server never declared (LIFE-009, held against the client).
if [ "$FAKE_CLIENT_VIOLATION" = logging ]; then
  curl -s -o /dev/null -X POST "$url" -H "mcp-session-id: $session" \
    --data '{"jsonrpc":"2.0","id":3,"method":"logging/setLevel","params":{"level":"info"}}'
fi
# The suite asking a server for a fixture by name: its own listing first, then
# a tool the stand-in server does not list (`FAKE_FIXTURE=test_simple_text`) or
# one it does (`FAKE_FIXTURE=echo`).
if [ -n "$FAKE_FIXTURE" ]; then
  curl -s -o /dev/null -X POST "$url" -H "mcp-session-id: $session" \
    --data '{"jsonrpc":"2.0","id":4,"method":"tools/list"}'
  curl -s -o /dev/null -X POST "$url" -H "mcp-session-id: $session" \
    --data '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"'"$FAKE_FIXTURE"'"}}'
fi
curl -s -o /dev/null -X DELETE "$url" -H "mcp-session-id: $session"
dir="$out/server-$scenario-2026-10-13T00-00-00-000Z"
mkdir -p "$dir"
printf '[{"id":"%s","status":"%s"}]\n' "$scenario" "$FAKE_STATUS" > "$dir/checks.json"
[ "$FAKE_STATUS" = SUCCESS ]
"#;

/// A stdio server answering `initialize` and `ping`.
const SERVER: &str = r#"while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"sh","version":"0"}}}' ;;
    *'"tools/list"'*) printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}' ;;
    *'"tools/call"'*) printf '%s\n' '{"jsonrpc":"2.0","id":5,"result":{"content":[{"type":"text","text":"nothing here"}],"isError":true}}' ;;
    *'"ping"'*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{}}' ;;
    *'"logging/setLevel"'*) printf '%s\n' '{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"Method not found"}}' ;;
  esac
done
"#;

struct Setup {
    dir: PathBuf,
    path: String,
    server: String,
}

fn setup(name: &str) -> Setup {
    let dir = scratch(name);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (file, body) in [("node", NODE), ("npx", NPX)] {
        let path = bin.join(file);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let server = dir.join("server.sh");
    std::fs::write(&server, SERVER).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Setup {
        path,
        server: format!("sh {}", server.display()),
        dir,
    }
}

fn run(setup: &Setup, status: &str, version: &str, extra: &[&str]) -> Output {
    run_with(setup, status, version, extra, &[])
}

/// [`run`], with `env` set for the stand-in suite as well.
fn run_with(
    setup: &Setup,
    status: &str,
    version: &str,
    extra: &[&str],
    env: &[(&str, &str)],
) -> Output {
    binary()
        .args([
            "test",
            "--command",
            &setup.server,
            "--format",
            "json",
            "--work-dir",
        ])
        .arg(setup.dir.join("work"))
        .args(extra)
        .env("PATH", &setup.path)
        .env("FAKE_STATUS", status)
        .env("FAKE_SUITE_VERSION", version)
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

#[test]
fn a_passing_stdio_server_exits_0_with_both_verdicts_reported() {
    let setup = setup("pass");
    let output = run(&setup, "SUCCESS", mcp_conformance_cli::suite::VERSION, &[]);
    let message = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{message}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verdict"], "pass", "{report:#}");
    assert_eq!(report["suite"]["passed"], true);
    let scenarios: Vec<&str> = report["suite"]["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scenario| scenario["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        scenarios,
        ["ping"],
        "the pending and HTTP-transport ones are not run"
    );
    let not_run: Vec<&str> = report["suite"]["not_run"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["scenario"].as_str().unwrap())
        .collect();
    assert_eq!(not_run, ["dns-rebinding-protection", "json-schema-2020-12"]);
    let traces = report["sessions"]["traces"].as_array().unwrap();
    assert_eq!(traces.len(), 1, "{report:#}");
    assert_eq!(report["sessions"]["verdict"], "pass");
    assert_eq!(report["agreement"]["holds"], true);
    assert!(message.contains("verdict: pass"), "{message}");
    // The JSON on stdout holds the suite's checks and the agreement in full, so
    // stderr says the verdicts and not the block again (see
    // `stderr_does_not_repeat_what_the_report_on_stdout_holds`).
    assert!(message.contains("suite verdict: pass"), "{message}");
    assert!(message.contains("agreement: holds"), "{message}");
    // The trace is the stdio wire, kept in the work directory.
    let trace = std::fs::read_to_string(setup.dir.join("work/traces/001.jsonl")).unwrap();
    assert!(trace.contains(r#""transport":"stdio""#), "{trace}");
    assert!(trace.contains(r#""method":"ping""#), "{trace}");
    std::fs::remove_dir_all(&setup.dir).ok();
}

/// The dogfood audit's repeated block, reproduced with the real output of the
/// Node reference server: `test` printed the suite's checks and the agreement on
/// stdout, in the report, and again on stderr. A reader of the terminal saw
/// both, one after the other. Where the report on stdout carries them, stderr is
/// the verdicts; where it does not (a file, or `junit` and `sarif`, which hold
/// only the clause report), stderr carries them whole.
#[test]
fn stderr_does_not_repeat_what_the_report_on_stdout_holds() {
    let run_format = |name: &str, format: &str, to_file: bool| {
        let setup = setup(name);
        let file = setup.dir.join("report.out");
        let mut command = binary();
        command
            .args(["test", "--command", &setup.server, "--format", format])
            .arg("--work-dir")
            .arg(setup.dir.join("work"))
            .env("PATH", &setup.path)
            .env("FAKE_STATUS", "SUCCESS")
            .env("FAKE_SUITE_VERSION", mcp_conformance_cli::suite::VERSION);
        if to_file {
            command.arg("-o").arg(&file);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        std::fs::remove_dir_all(&setup.dir).ok();
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr(&output),
        )
    };
    let listing = "== The official suite ==";
    let agreement = "failing clauses:";

    // Human or JSON on stdout: the report has the block, stderr the verdicts.
    for (format, in_report) in [
        ("human", [listing, agreement]),
        ("json", ["\"suite\"", "\"holds\""]),
    ] {
        let (out, err) = run_format(&format!("dedupe-{format}"), format, false);
        for needle in in_report {
            assert!(
                out.contains(needle),
                "{format}: {needle} not in the report:\n{out}"
            );
        }
        assert!(!err.contains(listing), "{format}: repeated:\n{err}");
        assert!(!err.contains(agreement), "{format}: repeated:\n{err}");
        assert!(err.contains("suite verdict: pass"), "{format}: {err}");
        assert!(
            err.contains("clause verdict (server findings only) over 1 judged session(s): pass"),
            "{format}: {err}"
        );
        assert!(err.contains("agreement: holds"), "{format}: {err}");
        assert!(
            err.contains("verdict: pass; suite results"),
            "{format}: {err}"
        );
    }

    // A file takes the report off the terminal: stderr has it whole.
    // `junit` and `sarif` hold only the clause report: so does stderr.
    for (format, to_file) in [
        ("human", true),
        ("json", true),
        ("junit", false),
        ("sarif", false),
    ] {
        let (_, err) = run_format(&format!("whole-{format}-{to_file}"), format, to_file);
        assert!(err.contains(listing), "{format}: {err}");
        assert!(err.contains(agreement), "{format}: {err}");
        assert!(err.contains("agreement: holds"), "{format}: {err}");
    }
}

/// The dogfood audit's finding 3.1: against a server that is not the suite's own
/// fixture server, scenarios fail and nothing says why. Here the stand-in suite
/// asks for a tool the server's own `tools/list` does not hold and fails its
/// scenario; the report says what the traffic shows, and only when it shows it.
#[test]
fn a_suite_asking_for_a_tool_the_server_does_not_list_is_noted_when_it_fails() {
    let note = "note: 1 of the suite's 1 scenario(s) failed. In 1 of the 1 recorded sessions the \
                suite asked for a tool, prompt or resource that none of this server's own list \
                answers in this run includes:\n  tools (1): test_simple_text\n";
    let setup = setup("fixtures");
    let output = run_with(
        &setup,
        "FAILURE",
        mcp_conformance_cli::suite::VERSION,
        &[],
        &[("FAKE_FIXTURE", "test_simple_text")],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["suite"]["fixtures"],
        serde_json::json!({
            "failed_scenarios": 1,
            "scenarios": 1,
            "sessions": 1,
            "judged_sessions": 1,
            "unlisted": {"tools": ["test_simple_text"], "prompts": [], "resources": []},
        }),
        "{report:#}"
    );
    std::fs::remove_dir_all(&setup.dir).ok();

    // In words, after the suite's own part.
    let setup = self::setup("fixtures-human");
    let output = binary()
        .args(["test", "--command", &setup.server, "--work-dir"])
        .arg(setup.dir.join("work"))
        .env("PATH", &setup.path)
        .env("FAKE_STATUS", "FAILURE")
        .env("FAKE_SUITE_VERSION", mcp_conformance_cli::suite::VERSION)
        .env("FAKE_FIXTURE", "test_simple_text")
        .output()
        .unwrap();
    let human = String::from_utf8_lossy(&output.stdout);
    let at = human
        .find(note)
        .unwrap_or_else(|| panic!("no note in:\n{human}"));
    assert!(
        human[..at].ends_with("suite verdict: fail\n"),
        "the note follows the suite's verdict: {human}"
    );
    assert!(human.contains("(`test_simple_text` is one)"), "{human}");
    std::fs::remove_dir_all(&setup.dir).ok();
}

/// No note when the suite passed, when the tool is listed, or when the traffic
/// holds no request for a fixture: the note claims what the requests show.
#[test]
fn no_fixture_note_without_a_failed_scenario_and_an_unlisted_request() {
    for (name, status, fixture) in [
        ("fixtures-passed", "SUCCESS", Some("test_simple_text")),
        ("fixtures-listed", "FAILURE", Some("echo")),
        ("fixtures-none", "FAILURE", None),
    ] {
        let setup = self::setup(name);
        let env: Vec<(&str, &str)> = fixture
            .map(|tool| ("FAKE_FIXTURE", tool))
            .into_iter()
            .collect();
        let output = run_with(
            &setup,
            status,
            mcp_conformance_cli::suite::VERSION,
            &[],
            &env,
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            report["suite"]["fixtures"],
            serde_json::Value::Null,
            "{name}"
        );
        std::fs::remove_dir_all(&setup.dir).ok();
    }
}

/// The suite's own client breaks a clause the server is not party to
/// (`logging/setLevel` on a server that never declared `logging`, LIFE-009): the
/// finding is the client's, the report shows it as such, and the server it was
/// driven against is not held to it (the dogfood audit's finding 1.1).
#[test]
fn the_suites_own_client_violation_is_shown_and_not_held_against_the_server() {
    let setup = setup("party");
    let output = run_with(
        &setup,
        "SUCCESS",
        mcp_conformance_cli::suite::VERSION,
        &[],
        &[("FAKE_CLIENT_VIOLATION", "logging")],
    );
    let message = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{message}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["agreement"]["party"], "server", "{report:#}");
    assert_eq!(report["agreement"]["holds"], true, "{report:#}");
    let divergences = &report["agreement"]["divergences"];
    assert_eq!(divergences["unexplained"], serde_json::json!([]));
    let set_aside = divergences["set_aside"].as_array().unwrap();
    assert_eq!(set_aside.len(), 1, "{divergences:#}");
    assert_eq!(set_aside[0]["requirement"], "LIFE-009");
    assert_eq!(set_aside[0]["party"], "client");
    // The session's verdict is the server's: clean. The finding is still there,
    // held against the client, and the clause row is the trace's fact.
    assert_eq!(report["sessions"]["verdict"], "pass", "{report:#}");
    assert_eq!(report["sessions"]["party"], "server");
    assert_eq!(report["sessions"]["findings"]["set_aside"], 1);
    let session = &report["sessions"]["traces"][0]["reports"][0];
    assert_eq!(session["party_view"], "server");
    assert_eq!(session["verdict"], "pass");
    let row = session["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "LIFE-009")
        .unwrap();
    assert_eq!(row["outcome"], "fail");
    assert_eq!(row["findings"][0]["party"], "client");
    std::fs::remove_dir_all(&setup.dir).ok();

    // The same session, in the words a person reads.
    let setup = self::setup("party-human");
    let output = binary()
        .args(["test", "--command", &setup.server, "--work-dir"])
        .arg(setup.dir.join("work"))
        .env("PATH", &setup.path)
        .env("FAKE_STATUS", "SUCCESS")
        .env("FAKE_SUITE_VERSION", mcp_conformance_cli::suite::VERSION)
        .env("FAKE_CLIENT_VIOLATION", "logging")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let human = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "[client] seq ",
        "verdict (server findings only): pass \u{2014} 1 client finding set aside",
        "agreement: holds",
        "held against the suite's own client",
    ] {
        assert!(human.contains(needle), "missing {needle:?} in:\n{human}");
    }
    std::fs::remove_dir_all(&setup.dir).ok();
}

#[test]
fn a_failing_suite_check_exits_1_and_every_format_is_written() {
    let setup = setup("fail");
    let output = run(&setup, "FAILURE", mcp_conformance_cli::suite::VERSION, &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verdict"], "fail");
    assert_eq!(report["suite"]["passed"], false);
    assert_eq!(
        report["agreement"]["holds"], true,
        "the validator found nothing"
    );
    std::fs::remove_dir_all(&setup.dir).ok();

    for (format, needle) in [
        ("human", "== Agreement =="),
        ("junit", "<testsuites "),
        ("sarif", r#""version": "2.1.0""#),
    ] {
        let setup = self::setup(&format!("format-{format}"));
        let file = setup.dir.join("report.out");
        let output = binary()
            .args(["test", "--command", &setup.server, "--format", format, "-o"])
            .arg(&file)
            .arg("--work-dir")
            .arg(setup.dir.join("work"))
            .env("PATH", &setup.path)
            .env("FAKE_STATUS", "SUCCESS")
            .env("FAKE_SUITE_VERSION", mcp_conformance_cli::suite::VERSION)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{format}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "-o takes the report off stdout");
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains(needle), "{format}: {written}");
        std::fs::remove_dir_all(&setup.dir).ok();
    }
}

#[test]
fn a_suite_reporting_another_version_is_refused() {
    let setup = setup("version");
    let output = run(&setup, "SUCCESS", "9.9.9", &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("refusing to run a suite other than the pinned one"),
        "{}",
        stderr(&output)
    );
    std::fs::remove_dir_all(&setup.dir).ok();
}

#[test]
fn an_unexplained_divergence_or_a_stale_entry_exits_1() {
    let setup = setup("stale");
    let baseline = setup.dir.join("baseline.json");
    std::fs::write(
        &baseline,
        r#"{"policy":"p","divergences":[{"requirement":"LIFE-003","class":"suite-bug","upstream":"https://e.example/1"}]}"#,
    )
    .unwrap();
    let output = run(
        &setup,
        "SUCCESS",
        mcp_conformance_cli::suite::VERSION,
        &["--divergences", baseline.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["agreement"]["holds"], false);
    assert_eq!(
        report["agreement"]["divergences"]["stale_baseline_entries"],
        serde_json::json!(["LIFE-003"])
    );
    std::fs::remove_dir_all(&setup.dir).ok();
}

#[test]
fn a_work_dir_in_use_or_a_bad_input_is_refused_before_anything_runs() {
    let setup = setup("refused");
    std::fs::create_dir_all(setup.dir.join("work")).unwrap();
    std::fs::write(setup.dir.join("work/kept"), "x").unwrap();
    let output = run(&setup, "SUCCESS", mcp_conformance_cli::suite::VERSION, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("is not empty"),
        "{}",
        stderr(&output)
    );
    let output = run(
        &setup,
        "SUCCESS",
        mcp_conformance_cli::suite::VERSION,
        &["--expected-failures", "/nonexistent.yaml"],
    );
    assert!(
        stderr(&output).contains("is not a file"),
        "{}",
        stderr(&output)
    );
    let output = run(
        &setup,
        "SUCCESS",
        mcp_conformance_cli::suite::VERSION,
        &["--divergences", "/nonexistent.json"],
    );
    assert!(
        stderr(&output).contains("cannot read /nonexistent.json"),
        "{}",
        stderr(&output)
    );
    std::fs::remove_dir_all(&setup.dir).ok();
}
