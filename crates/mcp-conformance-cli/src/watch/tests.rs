// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser as _;
use mcp_conformance_core::trace::Direction;
use mcp_trace_capture::{Recorder, stdio};

use super::judging::{self, Note, Setup};
use super::*;

/// A sink the test reads back.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn setup(out: &Shared, judge: judging::Judge) -> Setup {
    Setup {
        out: Box::new(out.clone()),
        terminal: false,
        numbered: false,
        name: Box::new(|_| "trace.jsonl".to_owned()),
        spans: None,
        peer: None,
        judge,
    }
}

/// The relay path is the recorder's `pump`, observed by `watch`'s tap; the
/// judge sleeps 50 ms a pass. The "server" writes 200 notifications (each a
/// reason to judge) 2 ms apart: judging inline, relaying them would take at
/// least 10 s; off the path it takes the 0.4 s the server takes, the judge
/// coalescing whatever queued behind each pass.
#[test]
fn a_slow_judge_never_slows_the_relay() {
    let out = Shared::default();
    let slow: judging::Judge = Box::new(|live| {
        std::thread::sleep(Duration::from_millis(50));
        live.judge()
    });
    let (tap, thread) = judging::start(setup(&out, slow));
    let notes = tap.0.clone();
    let recorder = Recorder::new(std::io::sink()).observed(Arc::new(tap), 1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let started = Instant::now();
    let unrecorded = runtime.block_on(async {
        let (mut server, from_server) = tokio::io::duplex(1 << 16);
        let writing = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;
            for n in 0..200 {
                let line = format!(
                    "{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{{\"level\":\"info\",\"data\":{n}}}}}\n"
                );
                server.write_all(line.as_bytes()).await.unwrap();
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        });
        let relayed = stdio::pump(
            &recorder,
            Direction::ServerToClient,
            from_server,
            tokio::io::sink(),
            1 << 20,
        )
        .await
        .unwrap();
        writing.await.unwrap();
        relayed
    });
    let relayed = started.elapsed();
    assert_eq!(unrecorded, stdio::Unrecorded::default());
    notes.send(Note::End).unwrap();
    let judged = thread.join().unwrap();
    assert!(
        relayed < Duration::from_secs(3),
        "the relay waited for judging: {relayed:?}"
    );
    assert_eq!(judged.lines, 200, "every line still judged");
    assert!(
        (1..100).contains(&judged.passes),
        "lines that queued behind a pass are judged together: {} passes for 200 \
         reasons to judge",
        judged.passes
    );
    let written = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
    assert!(
        written.contains("the validator's report of trace.jsonl"),
        "{written}"
    );
}

#[test]
fn findings_are_printed_with_the_tally_and_the_report_at_the_end() {
    let out = Shared::default();
    let (tap, thread) = judging::start(setup(
        &out,
        Box::new(mcp_conformance_cli::live::Live::judge),
    ));
    for line in [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#,
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#,
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
        r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"ping"}}"#,
        r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"error":{"code":-32603}}}"#,
    ] {
        mcp_trace_capture::Observer::observe(&tap, 1, line.as_bytes());
    }
    // The pass after the error response prints its finding before the end.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !String::from_utf8_lossy(&out.0.lock().unwrap()).contains("BASE-006") {
        assert!(Instant::now() < deadline, "no live finding");
        std::thread::sleep(Duration::from_millis(5));
    }
    tap.0.send(Note::End).unwrap();
    let judged = thread.join().unwrap();
    let written = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
    let live = written
        .split("the validator's report of trace.jsonl:")
        .next()
        .unwrap();
    assert_eq!(
        live.lines().next().unwrap(),
        "mcp-conformance watch: FAIL BASE-006 (MUST) seq 4: error object lacks a message member",
        "{written}"
    );
    assert!(
        live.contains("\nmcp-conformance watch: 3 exchange(s); findings so far: "),
        "a tally after the findings: {written}"
    );
    assert!(written.contains("verdict: fail"), "{written}");
    assert!(
        written.ends_with(&format!(
            "3 exchange(s); findings so far: {} fail, 0 warn\n",
            judged.findings
        )),
        "{written}"
    );
    assert_eq!((judged.lines, judged.withdrawn), (5, 0));
    assert!(!written.contains("withdrawn"));
}

/// Every `mcp-conformance watch …` line in `text`'s fenced `text` blocks.
fn documented(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        if line.starts_with("```") {
            in_block = line == "```text" && !in_block;
            continue;
        }
        if in_block && line.starts_with("mcp-conformance watch") {
            commands.push(line.to_owned());
        }
    }
    commands
}

/// Each `watch` command the README and the book show is one this binary
/// parses: the documentation cannot show a flag that does not exist.
#[test]
fn every_documented_watch_command_parses() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut seen = 0;
    for document in ["README.md", "../../book/src/watching.md"] {
        let text = std::fs::read_to_string(root.join(document)).unwrap();
        let commands = documented(&text);
        assert!(!commands.is_empty(), "{document} shows no watch command");
        for command in commands {
            let words: Vec<&str> = command.split_whitespace().collect();
            let parsed = crate::args::Cli::try_parse_from(&words);
            assert!(
                parsed.is_ok(),
                "{document}: `{command}`: {:?}",
                parsed.err()
            );
            seen += 1;
        }
    }
    assert!(seen >= 4, "{seen}");
}

#[test]
fn an_http_upstream_names_its_server_and_port() {
    assert_eq!(
        peer("http://localhost:3000/mcp"),
        Some(Peer::new("localhost", 3000))
    );
    assert_eq!(
        peer("https://example.com/mcp"),
        Some(Peer::new("example.com", 443))
    );
    assert_eq!(peer("http://[::1]/"), Some(Peer::new("::1", 80)));
    assert_eq!(peer("localhost:3000"), None, "refused by the recorder, too");
}
