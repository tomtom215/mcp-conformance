// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `watch stdio` over a corpus replay: a `sh` stand-in server answers each
//! client message with the server's messages the trace records after it, and
//! the test plays the client, one message at a time.
//!
//! **"Within one exchange", tested exactly.** The finding about the violating
//! message must be on `watch`'s stderr after that message reached the client
//! and *before the client sends anything more* — the test does not send the
//! next message until it has read the finding (or gives up after a timeout,
//! and fails). Then the session continues and ends normally.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

/// Replays numbered files: file `0` at start, file `k` after the k-th line read.
const REPLAY: &str = r#"[ -f "$1/0" ] && cat "$1/0"
i=0
while IFS= read -r line; do
  i=$((i+1))
  if [ -f "$1/$i" ]; then cat "$1/$i"; fi
done
"#;

/// The violation: `tools/list` answers with a tool name outside the allowed
/// characters (TOOL-006; seq 4 in the corpus file, seq 5 under the wrapper,
/// which records the transport opening first), and one more exchange the
/// test adds after it.
const CORPUS: &str = "../../corpus/violations/tool-006-name-charset.jsonl";
const AFTER: [&str; 2] = [
    r#"{"jsonrpc":"2.0","id":99,"method":"ping"}"#,
    r#"{"jsonrpc":"2.0","id":99,"result":{}}"#,
];

/// The client's messages, and for each the server's lines that follow it.
struct Replay {
    client: Vec<String>,
    dir: PathBuf,
    /// For each client message (1-based), how many lines the server answers.
    answers: Vec<usize>,
    script: PathBuf,
}

fn replay(name: &str) -> Replay {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let trace = std::fs::read_to_string(root.join(CORPUS)).unwrap();
    let dir = scratch(name);
    let groups = dir.join("groups");
    std::fs::create_dir_all(&groups).unwrap();
    let mut client = Vec::new();
    let mut server: Vec<Vec<String>> = vec![Vec::new()];
    let mut messages: Vec<(String, String)> = trace
        .lines()
        .map(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            (
                event["direction"].as_str().unwrap().to_owned(),
                event["payload"].to_string(),
            )
        })
        .collect();
    messages.push(("client-to-server".to_owned(), AFTER[0].to_owned()));
    messages.push(("server-to-client".to_owned(), AFTER[1].to_owned()));
    for (direction, payload) in messages {
        if direction == "client-to-server" {
            client.push(payload);
            server.push(Vec::new());
        } else {
            server.last_mut().unwrap().push(payload);
        }
    }
    for (index, lines) in server.iter().enumerate() {
        if !lines.is_empty() {
            let text = format!("{}\n", lines.join("\n"));
            std::fs::write(groups.join(index.to_string()), text).unwrap();
        }
    }
    assert!(
        server[0].is_empty(),
        "the server speaks first in no corpus trace used here"
    );
    let script = dir.join("replay.sh");
    std::fs::write(&script, REPLAY).unwrap();
    Replay {
        client,
        answers: server[1..].iter().map(Vec::len).collect(),
        dir,
        script,
    }
}

struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr: Receiver<String>,
    seen: Arc<Mutex<Vec<String>>>,
}

fn start(replay: &Replay, extra: &[&str]) -> Running {
    let trace = replay.dir.join("trace.jsonl");
    let mut child = binary()
        .arg("watch")
        .arg("-o")
        .arg(&trace)
        .args(extra)
        .args(["stdio", "--", "sh"])
        .arg(&replay.script)
        .arg(replay.dir.join("groups"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (lines, received) = mpsc::channel();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            kept.lock().unwrap().push(line.clone());
            let _ = lines.send(line);
        }
    });
    Running {
        stdin: child.stdin.take(),
        stdout: BufReader::new(child.stdout.take().unwrap()),
        child,
        stderr: received,
        seen,
    }
}

impl Running {
    fn send(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    fn read_answers(&mut self, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                let mut line = String::new();
                self.stdout.read_line(&mut line).unwrap();
                line
            })
            .collect()
    }

    /// Waits for a stderr line containing `needle`.
    fn wait_for(&self, needle: &str) -> Option<String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.stderr.recv_timeout(left) {
                Ok(line) if line.contains(needle) => return Some(line),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }

    /// Ends the session (the client closes stdin) and returns the exit code
    /// and everything on stderr.
    fn end(mut self) -> (Option<i32>, String) {
        drop(self.stdin.take());
        let mut rest = String::new();
        self.stdout.read_to_string(&mut rest).unwrap();
        let status = self.child.wait().unwrap();
        // The reader thread has the last lines once the pipe closes.
        std::thread::sleep(Duration::from_millis(200));
        let seen = self.seen.lock().unwrap().join("\n");
        (status.code(), seen)
    }
}

/// Plays the client: every message, waiting after the violating response for
/// the finding before going on. Returns the finding line.
fn play(running: &mut Running, replay: &Replay) -> Option<String> {
    let mut finding = None;
    for (index, message) in replay.client.iter().enumerate() {
        running.send(message);
        let answers = running.read_answers(replay.answers[index]);
        if answers
            .iter()
            .any(|answer| answer.contains("weather lookup,v2!"))
        {
            // The violating message has reached the client; nothing more has
            // been sent. The finding must arrive now.
            finding = running.wait_for("TOOL-006");
            assert!(
                finding.is_some(),
                "no finding before the next message: {:?}",
                running.seen.lock().unwrap()
            );
        }
    }
    finding
}

#[test]
fn a_corpus_violation_prints_before_the_client_sends_its_next_message() {
    let replay = replay("watch-replay");
    let mut running = start(&replay, &[]);
    let finding = play(&mut running, &replay).unwrap();
    assert_eq!(
        finding,
        "mcp-conformance watch: WARN TOOL-006 (SHOULD) seq 5: tool name \"weather lookup,v2!\" \
         contains characters outside A-Z, a-z, 0-9, underscore, hyphen, and dot: \" ,!\"",
        "the clause ID, level, seq and detail"
    );
    let (code, stderr) = running.end();
    assert_eq!(code, Some(0), "the server's exit code: {stderr}");
    assert_eq!(
        stderr.matches("WARN TOOL-006 (SHOULD) seq 5").count(),
        1,
        "printed once, live: {stderr}"
    );
    assert!(stderr.contains("    spec: \""), "{stderr}");
    let report = stderr
        .split("the validator's report of")
        .nth(1)
        .unwrap_or_else(|| panic!("no final report: {stderr}"));
    assert!(report.contains("  WARN  TOOL-006 (SHOULD)"), "{report}");
    assert!(report.contains("verdict: pass-with-warnings"), "{report}");
    assert!(
        stderr.ends_with("4 exchange(s); findings so far: 0 fail, 2 warn"),
        "the tally last: {stderr}"
    );
    assert!(!stderr.contains("withdrawn"), "{stderr}");
    // The book quotes this run's live output; every line it shows is a line
    // `watch` printed.
    let book = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../book/src/watching.md"),
    )
    .unwrap();
    let quoted = book
        .split("```console\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap();
    let printed: Vec<&str> = stderr.lines().collect();
    for line in quoted.lines() {
        assert!(
            printed.contains(&line),
            "the book quotes a line watch did not print:\n  {line}\n{stderr}"
        );
    }
    let trace = std::fs::read_to_string(replay.dir.join("trace.jsonl")).unwrap();
    assert_eq!(
        trace
            .lines()
            .filter(|line| line.contains(r#""kind":"message""#))
            .count(),
        7,
        "the recording is capture's: {trace}"
    );
    std::fs::remove_dir_all(&replay.dir).ok();
}

/// Each request's head and body, as the stand-in collector received them.
type Requests = Arc<Mutex<Vec<(String, String)>>>;

/// A stand-in collector: answers every request `200 {}` and keeps each one's
/// head and body.
fn collector() -> (String, Requests) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let kept = Arc::clone(&kept);
            std::thread::spawn(move || serve(stream, &kept));
        }
    });
    (format!("http://{address}"), requests)
}

fn serve(stream: std::net::TcpStream, kept: &Mutex<Vec<(String, String)>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    loop {
        let mut head = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            if line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        kept.lock()
            .unwrap()
            .push((head, String::from_utf8(body).unwrap()));
        let _ = writer.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}",
        );
    }
}

#[test]
fn spans_reach_a_collector_with_the_conventions_names_and_the_finding() {
    let replay = replay("watch-otlp");
    let (url, requests) = collector();
    let mut running = start(&replay, &["--otlp", &url]);
    play(&mut running, &replay);
    let (code, stderr) = running.end();
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.contains("exported 4 span(s)"), "{stderr}");
    let requests = requests.lock().unwrap().clone();
    assert!(!requests.is_empty(), "{stderr}");
    let mut spans = Vec::new();
    for (head, body) in &requests {
        assert!(head.starts_with("POST /v1/traces HTTP/1.1\r\n"), "{head}");
        assert!(
            head.to_ascii_lowercase()
                .contains("content-type: application/json"),
            "{head}"
        );
        let request: serde_json::Value = serde_json::from_str(body).unwrap();
        for span in request["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .unwrap()
        {
            spans.push(span.clone());
        }
    }
    let names: Vec<&str> = spans
        .iter()
        .map(|span| span["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "ping"
        ]
    );
    let trace_ids: std::collections::BTreeSet<&str> = spans
        .iter()
        .map(|span| span["traceId"].as_str().unwrap())
        .collect();
    assert_eq!(trace_ids.len(), 1, "one trace per session");
    let list = &spans[2];
    assert_eq!(list["kind"], 3);
    let event = &list["events"][0];
    assert_eq!(event["name"], "mcp_conformance.finding");
    assert!(
        event["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |attribute| attribute["key"] == "mcp_conformance.requirement.id"
                    && attribute["value"]["stringValue"] == "TOOL-006"
            ),
        "{event}"
    );
    std::fs::remove_dir_all(&replay.dir).ok();
}

#[test]
fn a_collector_that_is_down_costs_the_session_nothing() {
    let replay = replay("watch-otlp-down");
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let mut running = start(&replay, &["--otlp", &url]);
    let finding = play(&mut running, &replay);
    assert!(finding.is_some());
    let (code, stderr) = running.end();
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stderr.contains("exported 0 span(s)") && stderr.contains("4 not accepted (cannot reach"),
        "{stderr}"
    );
    std::fs::remove_dir_all(&replay.dir).ok();
}

/// The same replay against a real OpenTelemetry Collector, whose OTLP/HTTP
/// receiver must accept every span (a 2xx for each request). Run by CI's
/// `otlp` job, which starts the pinned collector and then reads its file
/// exporter's output for the conventions' names and attributes; ignored
/// elsewhere, since it needs that collector.
#[test]
#[ignore = "needs an OpenTelemetry Collector: set MCP_CONFORMANCE_OTLP_ENDPOINT (CI's otlp job)"]
fn spans_are_accepted_by_a_real_collector() {
    let url = std::env::var("MCP_CONFORMANCE_OTLP_ENDPOINT")
        .expect("MCP_CONFORMANCE_OTLP_ENDPOINT names the collector, e.g. http://127.0.0.1:4318");
    let replay = replay("watch-collector");
    let mut running = start(&replay, &["--otlp", &url]);
    assert!(play(&mut running, &replay).is_some());
    let (code, stderr) = running.end();
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stderr.contains("exported 4 span(s) to ") && !stderr.contains("not accepted"),
        "{stderr}"
    );
    std::fs::remove_dir_all(&replay.dir).ok();
}
