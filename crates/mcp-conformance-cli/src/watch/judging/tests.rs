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

/// A writer the test can read back.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

/// The audit's A12: the channel between the recorder and the judge was
/// unbounded, so a judge that fell behind held every line the session sent.
/// The queue holds a byte budget, drops what arrives past it and counts it,
/// and never waits: the session's path stays what it was.
#[test]
fn a_queue_past_its_budget_drops_lines_and_counts_them_without_waiting() {
    let (tap, notes, queue) = channel(100);
    for _ in 0..10 {
        tap.observe(1, &[b'x'; 30]);
    }
    // 0, 30, 60 and 90 bytes held: four lines fit before the budget is spent,
    // the fourth taking it past 100 (a line is never split).
    assert_eq!(notes.try_iter().count(), 4);
    assert_eq!(queue.lost.load(Ordering::Relaxed), 6);
    assert_eq!(queue.held.load(Ordering::Relaxed), 120);

    // The end is never dropped: it is what stops the judge.
    tap.notes.send(Note::End).unwrap();
    assert!(matches!(notes.try_recv(), Ok(Note::End)));
}

/// What the judge takes off the queue is room for more, and a dropped line
/// does not count against the budget.
#[test]
fn what_the_judge_takes_makes_room_for_the_lines_after_it() {
    // A notification line is a little over 120 bytes: two fit under 200.
    let (tap, notes, queue) = channel(200);
    let mut thread = Thread::with_queue(setup(Box::new(Live::judge)), queue.clone());
    for seq in 0..3 {
        tap.observe(1, &notification(seq));
    }
    assert_eq!(
        queue.lost.load(Ordering::Relaxed),
        1,
        "the third found it spent"
    );
    let held = queue.held.load(Ordering::Relaxed);
    assert_eq!(
        held,
        2 * notification(0).len(),
        "a dropped line is not held"
    );
    while let Ok(Note::Line { trace, at, line }) = notes.try_recv() {
        queue.release(line.len());
        thread.take(trace, at, &line);
    }
    assert_eq!(queue.held.load(Ordering::Relaxed), 0);
    assert_eq!(thread.judged.lines, 2);
    tap.observe(1, &notification(3));
    assert_eq!(notes.try_iter().count(), 1, "room again");
    assert_eq!(queue.lost.load(Ordering::Relaxed), 1);
}

/// A run that lost lines says so, and judges the trace file at the end: the
/// judge's own copy has holes, so its final judgment would be of something
/// the session did not say.
#[test]
fn a_run_that_lost_lines_says_so_and_judges_the_trace_file() {
    let dir = std::env::temp_dir().join(format!("mcp-conformance-lossy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("trace.jsonl");
    // The whole recording, as the recorder wrote it: a request id reused.
    let whole: Vec<Vec<u8>> = vec![
        line(
            0,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}"#,
        ),
        line(
            1,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}"#,
        ),
        notification(2),
        line(
            3,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        ),
        line(
            4,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        ),
    ];
    let text: String = whole
        .iter()
        .map(|line| String::from_utf8(line.clone()).unwrap() + "\n")
        .collect();
    std::fs::write(&file, text).unwrap();

    let out = Captured::default();
    let name = file.display().to_string();
    let mut run = setup(Box::new(Live::judge));
    run.out = Box::new(out.clone());
    run.name = Box::new(move |_| name.clone());
    let queue = std::sync::Arc::new(Queue::default());
    let mut thread = Thread::with_queue(run, queue.clone());
    // The judge saw one line of five: the queue dropped the rest.
    thread.take(1, SystemTime::now(), &whole[0]);
    queue.lost.store(4, Ordering::Relaxed);
    thread.pass();
    thread.finish();
    let said = out.text();
    assert!(
        said.contains("4 line(s) were not judged live")
            && said.contains("the final report judges the trace file"),
        "{said}"
    );
    assert!(
        said.contains("BASE-003"),
        "the file's whole recording is judged: {said}"
    );
    assert_eq!(said.matches("4 line(s) were not judged live").count(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// A run that lost nothing says nothing about loss and judges as it did.
#[test]
fn a_run_that_lost_nothing_is_silent_about_it() {
    let out = Captured::default();
    let mut run = setup(Box::new(Live::judge));
    run.out = Box::new(out.clone());
    let mut thread = Thread::new(run);
    thread.take(1, SystemTime::now(), &notification(0));
    thread.pass();
    thread.finish();
    assert!(!out.text().contains("not judged live"), "{}", out.text());
}
