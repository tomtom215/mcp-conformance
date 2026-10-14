// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The judging thread's bookkeeping, driven a step at a time: what it takes
//! off the channel, when it judges, and what the end counts.

use std::time::Duration;

use mcp_conformance_cli::live::LiveFinding;

use super::*;

fn line(seq: u64, direction: &str, payload: &str) -> Vec<u8> {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
    )
    .into_bytes()
}

fn notification(seq: u64) -> Vec<u8> {
    line(
        seq,
        "client-to-server",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    )
}

fn setup(judge: Judge) -> Setup {
    Setup {
        out: Box::new(std::io::sink()),
        terminal: false,
        numbered: false,
        name: Box::new(|_| "trace.jsonl".to_owned()),
        spans: None,
        peer: None,
        judge,
    }
}

fn noted(seq: u64) -> Note {
    Note::Line {
        trace: 1,
        at: SystemTime::now(),
        line: notification(seq),
    }
}

#[test]
fn what_queued_before_the_end_is_taken_in_one_go_and_nothing_after_it() {
    let (notes, received) = mpsc::channel();
    for note in [noted(0), noted(1), Note::End, noted(2)] {
        notes.send(note).unwrap();
    }
    drop(notes);
    let judged = Thread::new(setup(Box::new(Live::judge))).run(&received);
    assert_eq!(judged.lines, 2, "the line after the end is not taken");
    // Both lines were due, but they arrived with the end: the final judgment
    // covers them, and no pass runs first.
    assert_eq!(judged.passes, 0);
}

#[test]
fn the_next_pass_waits_for_three_times_as_long_as_one_took() {
    let mut thread = Thread::new(setup(Box::new(|live| {
        std::thread::sleep(Duration::from_millis(20));
        live.judge()
    })));
    thread.take(1, SystemTime::now(), &notification(0));
    thread.pass();
    assert_eq!(thread.judged.passes, 1);
    assert!(
        thread.next_pass > Instant::now(),
        "a 20 ms pass puts the next one 60 ms after it"
    );
}

#[test]
fn the_end_counts_the_findings_only_the_whole_trace_shows() {
    let mut thread = Thread::new(setup(Box::new(Live::judge)));
    let initialize = line(
        0,
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}"#,
    );
    let initialized = line(
        1,
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}"#,
    );
    thread.take(1, SystemTime::now(), &initialize);
    thread.take(1, SystemTime::now(), &initialized);
    thread.finish();
    // LIFE-003: the client never sent notifications/initialized.
    assert_eq!(thread.judged.findings, 1);
    assert_eq!(thread.judged.withdrawn, 0);
}

#[test]
fn the_end_counts_what_it_withdraws() {
    let mut thread = Thread::new(setup(Box::new(Live::judge)));
    // Before any revision is declared the prefix is judged at the newest...
    thread.take(
        1,
        SystemTime::now(),
        &line(
            0,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        ),
    );
    thread.take(
        1,
        SystemTime::now(),
        &line(
            1,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603}}"#,
        ),
    );
    thread.pass();
    let live = thread.judged.findings;
    assert!(live > 0, "the prefix has findings");
    // ...and once one is, the whole trace is judged at it alone: every
    // finding reported at the newest is withdrawn.
    thread.take(
        1,
        SystemTime::now(),
        &line(
            2,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}"#,
        ),
    );
    thread.finish();
    assert_eq!(thread.judged.withdrawn, live);
}

/// A real finding: a tool name outside the allowed characters (TOOL-006).
fn a_finding() -> LiveFinding {
    let mut live = Live::new().unwrap();
    for text in [
        line(
            0,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}"#,
        ),
        line(
            1,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}"#,
        ),
        notification(2),
        line(
            3,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ),
        line(
            4,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"bad name!","inputSchema":{"type":"object"}}]}}"#,
        ),
    ] {
        live.push(&text, SystemTime::now());
    }
    live.judge()
        .new
        .into_iter()
        .find(|finding| finding.requirement == "TOOL-006")
        .unwrap()
}

#[test]
fn findings_before_any_exchange_are_counted_as_in_no_span() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let (sink, _exporting) = super::super::export::start(
        super::super::export::Endpoint::parse("http://127.0.0.1:9").unwrap(),
    );
    let found = a_finding();
    let mut other = found.clone();
    other.seq = Some(99);
    let mut thread = Thread::new(Setup {
        spans: Some(sink),
        ..setup(Box::new(move |_| {
            let mut pass = Pass::default();
            pass.new = vec![found.clone(), other.clone()];
            pass
        }))
    });
    // A message that is neither a request nor a response: a reason to judge
    // that completes no exchange.
    thread.take(
        1,
        SystemTime::now(),
        &line(0, "client-to-server", r#"{"jsonrpc":"2.0"}"#),
    );
    thread.pass();
    assert_eq!(thread.judged.findings, 2);
    thread.finish();
    // No exchange ever completed: every finding, the pass's two and any the
    // final judgment added, is in no span.
    assert!(thread.judged.findings >= 2);
    assert_eq!(thread.judged.unattached, thread.judged.findings);
}
