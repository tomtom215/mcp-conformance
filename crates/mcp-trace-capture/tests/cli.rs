// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The binary as a client launches it: stdio relayed byte for byte through a real
//! child process, the server's exit code passed through, and the refusals.
//!
//! Unix-only where a child is needed: `cat` and `sh` are the portable stand-ins for
//! a server. The wrapper around the real everything server runs in
//! `cargo xtask conformance`.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-{name}-{}.jsonl",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// Runs `command` with `input` on its stdin, collecting its output. The input is
/// written from its own thread: written first, anything larger than the pipe
/// buffers would deadlock against a child that echoes it.
fn run_with_stdin(mut command: Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    output
}

#[cfg(unix)]
#[test]
fn stdio_relays_bytes_unchanged_and_records_both_directions() {
    let trace = scratch("echo");
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
    let mut command = binary();
    command.args(["-o", trace.to_str().unwrap(), "stdio", "--", "cat"]);
    let output = run_with_stdin(command, input);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout, input,
        "the client sees exactly what the server wrote"
    );

    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    let events: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // open, two requests, two echoes, close — and every line a valid trace event.
    assert_eq!(events.len(), 6, "{text}");
    assert_eq!(events[0]["event"], "transport-open");
    assert_eq!(events[5]["event"], "transport-close");
    let directions: Vec<&str> = events[1..5]
        .iter()
        .map(|event| event["direction"].as_str().unwrap())
        .collect();
    assert_eq!(
        directions
            .iter()
            .filter(|d| **d == "client-to-server")
            .count(),
        2
    );
    for event in &events {
        serde_json::from_value::<mcp_conformance_core::trace::TraceEvent>(event.clone()).unwrap();
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("recorded 6 event(s)"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn the_servers_exit_code_passes_through_and_its_failure_is_an_abort() {
    let trace = scratch("exit");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "stdio",
        "--",
        "sh",
        "-c",
        "exit 7",
    ]);
    let output = run_with_stdin(command, b"");
    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(7), "{output:?}");
    assert!(text.contains(r#""event":"transport-abort""#), "{text}");
}

#[cfg(unix)]
#[test]
fn non_json_server_output_is_forwarded_and_reported_not_recorded() {
    let trace = scratch("notjson");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "stdio",
        "--",
        "sh",
        "-c",
        "echo 'log line on stdout'; echo '{\"jsonrpc\":\"2.0\",\"method\":\"x\"}'",
    ]);
    let output = run_with_stdin(command, b"");
    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "log line on stdout\n{\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n"
    );
    assert!(!text.contains("log line"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("1 server message(s) were not JSON"),
        "{stderr}"
    );
    assert!(!stderr.contains("size limit"), "{stderr}");
}

/// The server exits while the client still holds its end open — the session ends
/// by cancelling the client's relay — and what the client sent that the trace
/// could not hold is still reported. The count is taken before the line is
/// forwarded, and the server exits only after reading it, so the order is fixed.
#[cfg(unix)]
#[test]
fn client_side_counts_survive_a_server_that_exits_first() {
    let trace = scratch("server-first");
    let mut child = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "stdio",
            "--",
            "sh",
            "-c",
            "read line; exit 0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"not json\n").unwrap();
    // `stdin` stays open until the wrapper has exited.
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("1 client message(s) were not JSON"),
        "{stderr}"
    );
    // The server closed the session, and that is the trace's last word.
    let last = text.lines().last().unwrap();
    assert!(
        last.contains("\"server-to-client\"") && last.contains("transport-close"),
        "{text}"
    );
}

#[test]
fn an_existing_trace_is_not_overwritten_without_force() {
    let trace = scratch("exists");
    std::fs::write(&trace, "keep me\n").unwrap();
    let output = binary()
        .args(["-o", trace.to_str().unwrap(), "stdio", "--", "true"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(std::fs::read_to_string(&trace).unwrap(), "keep me\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--force"));
    std::fs::remove_file(&trace).ok();
}

#[test]
fn a_server_that_cannot_start_is_a_usage_error() {
    let trace = scratch("missing");
    let output = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "stdio",
            "--",
            "definitely-not-a-real-mcp-server-binary",
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot start"));
}

#[cfg(unix)]
#[test]
fn a_server_killed_by_a_signal_exits_128_plus_the_signal() {
    let trace = scratch("signal");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "stdio",
        "--",
        "sh",
        "-c",
        "kill -TERM $$",
    ]);
    let output = run_with_stdin(command, b"");
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(128 + 15), "{output:?}");
}

#[cfg(unix)]
#[test]
fn oversized_and_non_json_counts_are_reported_only_when_non_zero() {
    let trace = scratch("oversized");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "--max-message-bytes",
        "16",
        "stdio",
        "--",
        "cat",
    ]);
    let output = run_with_stdin(
        command,
        b"{\"a\":\"far more than sixteen bytes\"}\n{\"b\":1}\n",
    );
    std::fs::remove_file(&trace).ok();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The long line crossed in both directions; nothing was non-JSON.
    assert!(
        stderr.contains("1 client message(s) were over the size limit"),
        "{stderr}"
    );
    assert!(
        stderr.contains("1 server message(s) were over the size limit"),
        "{stderr}"
    );
    assert!(!stderr.contains("were not JSON"), "{stderr}");
}

#[cfg(target_os = "linux")]
#[test]
fn an_incomplete_trace_exits_3_unless_the_server_already_failed() {
    // /dev/full accepts the open and fails every write, as a full disk does.
    let mut command = binary();
    command.args(["--force", "-o", "/dev/full", "stdio", "--", "true"]);
    let output = run_with_stdin(command, b"");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("is incomplete"));

    let mut command = binary();
    command.args([
        "--force",
        "-o",
        "/dev/full",
        "stdio",
        "--",
        "sh",
        "-c",
        "exit 7",
    ]);
    let output = run_with_stdin(command, b"");
    assert_eq!(
        output.status.code(),
        Some(7),
        "the server's failure outranks the trace's"
    );
}

/// Starts the proxy with `args`, returns it and its listening address.
#[cfg(unix)]
/// A running proxy, killed if a test ends without stopping it — so a failed
/// assertion (or a mutant that ignores the signal) cannot leave it running.
struct Proxy(std::process::Child);

impl Drop for Proxy {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn start_proxy(
    trace: &std::path::Path,
    upstream: &str,
) -> (Proxy, String, std::sync::mpsc::Receiver<String>) {
    use std::io::BufRead as _;
    let mut child = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "http",
            "--listen",
            "127.0.0.1:0",
            "--upstream",
            upstream,
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (address_tx, address_rx) = std::sync::mpsc::channel();
    let (lines_tx, lines_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(rest) = line.split("listening on http://").nth(1) {
                let _ = address_tx.send(rest.split(',').next().unwrap().to_owned());
            }
            let _ = lines_tx.send(line);
        }
    });
    let address = address_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the proxy announces its address");
    (Proxy(child), address, lines_rx)
}

#[cfg(unix)]
fn interrupt(proxy: &mut Proxy) -> std::process::ExitStatus {
    let status = Command::new("kill")
        .args(["-INT", &proxy.0.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(status) = proxy.0.try_wait().unwrap() {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the proxy did not stop within 20 s of SIGINT"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn http_mode_stops_cleanly_and_reports_unreachable_upstreams() {
    use std::io::{Read as _, Write as _};
    // Nothing listens on this port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let upstream = format!("http://127.0.0.1:{port}");

    // A run with no traffic: a clean stop, exit 0, and no failure summary.
    let trace = scratch("http-idle");
    let (mut child, _, lines) = start_proxy(&trace, &upstream);
    assert_eq!(interrupt(&mut child).code(), Some(0));
    let stderr: Vec<String> = lines.try_iter().collect();
    assert!(
        !stderr.iter().any(|line| line.contains("could not reach")),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();

    // A request the upstream cannot answer: 502 from the proxy, and the summary.
    let trace = scratch("http-dead");
    let (mut child, address, lines) = start_proxy(&trace, &upstream);
    let mut stream = std::net::TcpStream::connect(&address).unwrap();
    stream
        .write_all(b"POST /mcp HTTP/1.1\r\nhost: localhost\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
    assert_eq!(interrupt(&mut child).code(), Some(0));
    std::thread::sleep(std::time::Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    assert!(
        stderr
            .iter()
            .any(|line| line.contains("1 request(s) could not reach the upstream")),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();
}

#[test]
fn http_mode_rejects_an_upstream_that_is_not_an_absolute_http_url() {
    let trace = scratch("http-bad");
    let output = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "http",
            "--upstream",
            "ftp://example.com",
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not an absolute http"));
}

/// The `mcp-trace-capture` command lines the root README's quickstart shows.
fn readme_quickstart_commands() -> Vec<Vec<String>> {
    let readme =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md")).unwrap();
    let start = readme
        .find("## Quickstart")
        .expect("README has a quickstart");
    let end = readme[start..]
        .find("\n## ")
        .map_or(readme.len(), |at| start + at);
    readme[start..end]
        .lines()
        .filter(|line| line.starts_with("mcp-trace-capture "))
        .map(|line| {
            line.split_whitespace()
                .skip(1)
                .map(ToOwned::to_owned)
                .collect()
        })
        .collect()
}

#[test]
fn every_quickstart_command_in_the_readme_is_valid_for_this_cli() {
    let commands = readme_quickstart_commands();
    assert_eq!(
        commands.len(),
        2,
        "one stdio and one http example: {commands:?}"
    );
    for mut args in commands {
        // `--help` after the subcommand's own arguments makes clap validate every
        // flag and value the README shows, then exit 0 without running anything.
        // It goes before any `--`, which hands everything after it to the server.
        let at = args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(args.len());
        args.insert(at, "--help".to_owned());
        let output = binary()
            .args(&args)
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "README command {args:?}: {output:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn the_readme_stdio_quickstart_records_a_session() {
    let trace = scratch("readme-stdio");
    let args = readme_quickstart_commands()
        .into_iter()
        .find(|args| args.contains(&"stdio".to_owned()))
        .unwrap();
    // The README's command, with `cat` standing in for its example server and the
    // trace written somewhere this test owns.
    let separator = args.iter().position(|arg| arg == "--").unwrap();
    let mut command = binary();
    command.args(["--force", "-o", trace.to_str().unwrap()]);
    let mut own = args[..separator].iter();
    while let Some(arg) = own.next() {
        if arg == "-o" {
            own.next(); // the README's output path; this test writes its own
        } else {
            command.arg(arg);
        }
    }
    command.args(["--", "cat"]);
    let output = run_with_stdin(
        command,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    );
    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        text.lines().count(),
        4,
        "open, request, echo, close: {text}"
    );
}

/// The defaults agree end to end: a message just under the capture's default limit
/// is recorded, and the trace reads back under the validator's default limits. A
/// message within the limit whose line would grow past it — `9e15` is written
/// back as `9000000000000000.0` — is forwarded, counted, and kept out of the trace
/// rather than making the trace unreadable.
#[cfg(unix)]
#[test]
fn a_trace_recorded_at_the_default_limits_reads_back_under_the_validators() {
    use mcp_trace_validator::reader::{Limits, parse_trace};

    let trace = scratch("default-limits");
    let head = r#"{"jsonrpc":"2.0","method":"x","params":{"s":""#;
    let tail = "\"}}";
    let fill = mcp_trace_capture::DEFAULT_MAX_MESSAGE - head.len() - tail.len();
    let mut input = format!("{head}{}{tail}\n", "a".repeat(fill)).into_bytes();
    assert_eq!(input.len(), mcp_trace_capture::DEFAULT_MAX_MESSAGE + 1);
    // 20 MiB of numbers read, over 70 MiB written.
    let numbers = vec!["9e15"; 20 * 1024 * 1024 / 5].join(",");
    input.extend_from_slice(format!("[{numbers}]\n").as_bytes());

    let mut command = binary();
    command.args(["-o", trace.to_str().unwrap(), "stdio", "--", "cat"]);
    let output = run_with_stdin(command, &input);
    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    assert!(output.status.success(), "{:?}", output.status);
    assert_eq!(output.stdout.len(), input.len(), "every byte is forwarded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("1 client message(s) were over the size limit"),
        "{stderr}"
    );
    assert!(
        stderr.contains("1 server message(s) were over the size limit"),
        "{stderr}"
    );

    let events = parse_trace(&text, &Limits::default()).expect("readable at the defaults");
    let messages = events
        .iter()
        .filter(|event| event.message_payload().is_some())
        .count();
    // The large message, in each direction; the numbers in neither.
    assert_eq!(messages, 2);
    let longest = text.lines().map(str::len).max().unwrap();
    assert!(
        longest > mcp_trace_capture::DEFAULT_MAX_MESSAGE,
        "{longest}"
    );
    assert!(longest <= Limits::default().max_line_bytes, "{longest}");
}

/// A message limit above the default makes lines the validator's default refuses
/// possible, and the capture names the flag and value to validate with — only then.
#[cfg(unix)]
#[test]
fn a_raised_message_limit_names_the_validator_flag_to_match() {
    let trace = scratch("raised-limit");
    let run = |limit: Option<&str>| {
        let mut command = binary();
        command.args(["-o", trace.to_str().unwrap(), "--force"]);
        if let Some(limit) = limit {
            command.args(["--max-message-bytes", limit]);
        }
        command.args(["stdio", "--", "cat"]);
        let output = run_with_stdin(command, b"{}\n");
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let raised = run(Some("70000000"));
    assert!(
        raised.contains("mcp-trace-validator validate --max-line-bytes 71048576"),
        "{raised}"
    );
    let at_default = run(Some(&mcp_trace_capture::DEFAULT_MAX_MESSAGE.to_string()));
    assert!(!at_default.contains("--max-line-bytes"), "{at_default}");
    let unset = run(None);
    std::fs::remove_file(&trace).ok();
    assert!(!unset.contains("--max-line-bytes"), "{unset}");
}
