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
curl -s -o /dev/null -X DELETE "$url" -H "mcp-session-id: $session"
dir="$out/server-$scenario-2026-10-13T00-00-00-000Z"
mkdir -p "$dir"
printf '[{"id":"%s","status":"%s"}]\n' "$scenario" "$FAKE_STATUS" > "$dir/checks.json"
[ "$FAKE_STATUS" = SUCCESS ]
"#;

/// A stdio server answering `initialize` and `ping`.
const SERVER: &str = r#"while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"sh","version":"0"}}}' ;;
    *'"ping"'*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{}}' ;;
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
    assert!(message.contains("== The official suite =="), "{message}");
    // The trace is the stdio wire, kept in the work directory.
    let trace = std::fs::read_to_string(setup.dir.join("work/traces/001.jsonl")).unwrap();
    assert!(trace.contains(r#""transport":"stdio""#), "{trace}");
    assert!(trace.contains(r#""method":"ping""#), "{trace}");
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
