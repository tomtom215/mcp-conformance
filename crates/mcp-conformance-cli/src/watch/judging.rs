// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The judging thread: off the session's path, always.
//!
//! The recorder shows each line to a [`Tap`], which copies it onto a channel
//! and returns — the only work `watch` adds to forwarding a message. A thread
//! of its own takes lines off the channel, pairs them into exchanges, and
//! judges. If judging falls behind, lines wait in the channel and the next pass
//! covers all of them: the findings lag, the session never does.
//!
//! **Bounded, and loud about it.** The channel holds at most [`QUEUE_BYTES`]
//! of lines (a line is never split, so it can pass the budget by one). A line
//! that arrives while the budget is spent is dropped and counted, never waited
//! for, and the run says how many (the judge's live view then has holes). The
//! trace file has every line either way, so a run that lost lines judges the
//! file at the end instead of the judge's own copy.
//!
//! **Pacing.** A pass re-judges the whole trace so far, so its cost grows with
//! the session (measured in the crate README: about 0.17 ms at 41 events, 80
//! ms at 10,001). After a pass that took `d`, the next waits until `3 × d`
//! has passed, lines arriving meanwhile coalescing into it: judging takes at
//! most a quarter of one core. While passes are cheap that is no wait at all
//! — every exchange gets its own pass, and a finding prints as the exchange
//! that shows it completes; on a 10,000-message session it is a pass about
//! every 320 ms.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Instant, SystemTime};

use mcp_conformance_cli::live::{Exchange, Live, Pass};
use mcp_conformance_cli::otlp::{Peer, Spans};
use mcp_trace_capture::Observer;
use mcp_trace_validator::judge;

use super::export::SpanSink;
use super::ids::Ids;
use super::print::{self, Tally};

/// What the recorder hands the judging thread.
#[derive(Debug)]
pub(super) enum Note {
    /// A line written to trace `trace` at `at`.
    Line {
        trace: u64,
        at: SystemTime,
        line: Vec<u8>,
    },
    /// The recording is over.
    End,
}

/// The most bytes of trace lines the queue between the recorder and the judge
/// holds: what a judge that fell behind costs in memory, however long the
/// session runs.
pub(super) const QUEUE_BYTES: usize = 64 * 1024 * 1024;

/// What the queue holds and has dropped, shared by the tap and the judge.
#[derive(Debug, Default)]
pub(super) struct Queue {
    /// Bytes of lines sent and not yet taken.
    held: AtomicUsize,
    /// Lines dropped because the queue held its budget.
    lost: AtomicU64,
}

impl Queue {
    /// A line of `bytes` has been taken off the queue.
    fn release(&self, bytes: usize) {
        self.held.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// The recorder's observer: a bounded channel, nothing more.
#[derive(Debug)]
pub(super) struct Tap {
    /// Where the lines go; [`Note::End`] is sent here too and is never
    /// counted against the budget.
    pub(super) notes: Sender<Note>,
    queue: Arc<Queue>,
    limit: usize,
}

impl Observer for Tap {
    fn observe(&self, trace: u64, line: &[u8]) {
        let before = self.queue.held.fetch_add(line.len(), Ordering::Relaxed);
        if before >= self.limit {
            self.queue.held.fetch_sub(line.len(), Ordering::Relaxed);
            self.queue.lost.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let sent = self.notes.send(Note::Line {
            trace,
            at: SystemTime::now(),
            line: line.to_vec(),
        });
        if sent.is_err() {
            self.queue.release(line.len());
        }
    }
}

/// A tap, the end of its channel, and the queue's counters, for a budget of
/// `limit` bytes.
fn channel(limit: usize) -> (Tap, Receiver<Note>, Arc<Queue>) {
    let (notes, received) = mpsc::channel();
    let queue = Arc::new(Queue::default());
    let tap = Tap {
        notes,
        queue: Arc::clone(&queue),
        limit,
    };
    (tap, received, queue)
}

/// A judging pass over one session.
pub(super) type Judge = Box<dyn FnMut(&mut Live) -> Pass + Send>;

/// How the thread reports and exports.
pub(super) struct Setup {
    /// Where findings, the tally and the reports go (stderr).
    pub(super) out: Box<dyn Write + Send>,
    /// Rewrite the tally in place (a terminal), rather than print it as lines.
    pub(super) terminal: bool,
    /// Name each session in finding lines (a trace per session).
    pub(super) numbered: bool,
    /// The trace's name for the final report, by trace number.
    pub(super) name: Box<dyn Fn(u64) -> String + Send>,
    /// Where spans go, with `--otlp`.
    pub(super) spans: Option<SpanSink>,
    /// The HTTP session's server, for spans.
    pub(super) peer: Option<Peer>,
    /// The judging pass.
    pub(super) judge: Judge,
}

/// What the thread did, for the end of the run and for tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Judged {
    pub(super) lines: u64,
    pub(super) passes: u64,
    pub(super) findings: u64,
    pub(super) withdrawn: u64,
    /// Findings no span could carry (`--otlp`).
    pub(super) unattached: u64,
}

struct Session {
    live: Option<Live>,
    completed: Vec<Exchange>,
    due: bool,
    spans: Option<Spans>,
}

/// Starts the thread; send it [`Note::End`] after the recording ends.
pub(super) fn start(setup: Setup) -> (Tap, std::thread::JoinHandle<Judged>) {
    let (tap, received, queue) = channel(QUEUE_BYTES);
    let thread = std::thread::spawn(move || Thread::with_queue(setup, queue).run(&received));
    (tap, thread)
}

struct Thread {
    setup: Setup,
    ids: Ids,
    sessions: BTreeMap<u64, Session>,
    tally: Tally,
    judged: Judged,
    next_pass: Instant,
    queue: Arc<Queue>,
    /// How many dropped lines the run has already said so about.
    said_lost: u64,
}

impl Thread {
    #[cfg(test)]
    fn new(setup: Setup) -> Self {
        Self::with_queue(setup, Arc::new(Queue::default()))
    }

    fn with_queue(setup: Setup, queue: Arc<Queue>) -> Self {
        Self {
            setup,
            ids: Ids::new(),
            sessions: BTreeMap::new(),
            tally: Tally::default(),
            judged: Judged::default(),
            next_pass: Instant::now(),
            queue,
            said_lost: 0,
        }
    }

    /// The lines the queue has dropped, said once each time the count grows:
    /// the judge's live view has holes, and the final report judges the file.
    fn loss_notice(&mut self) -> String {
        let lost = self.queue.lost.load(Ordering::Relaxed);
        if lost == self.said_lost {
            return String::new();
        }
        self.said_lost = lost;
        format!(
            "{} the judge fell behind: {lost} line(s) were not judged live (the queue holds \
             {} MiB); the trace file holds them, and the final report judges the trace file\n",
            print::PREFIX,
            QUEUE_BYTES / (1024 * 1024)
        )
    }

    fn run(mut self, notes: &Receiver<Note>) -> Judged {
        let mut ended = false;
        while !ended {
            let due = self.sessions.values().any(|session| session.due);
            let note = if due {
                let wait = self.next_pass.saturating_duration_since(Instant::now());
                match notes.recv_timeout(wait) {
                    Ok(note) => Some(note),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => Some(Note::End),
                }
            } else {
                Some(notes.recv().unwrap_or(Note::End))
            };
            let mut next = note;
            while let Some(note) = next.take() {
                match note {
                    Note::Line { trace, at, line } => {
                        self.queue.release(line.len());
                        self.take(trace, at, &line);
                    }
                    Note::End => ended = true,
                }
                if !ended {
                    next = notes.try_recv().ok();
                }
            }
            if !ended && Instant::now() >= self.next_pass {
                self.pass();
            }
        }
        self.finish();
        self.judged
    }

    fn take(&mut self, trace: u64, at: SystemTime, text: &[u8]) {
        self.judged.lines += 1;
        let ids = &self.ids;
        let exporting = self.setup.spans.is_some();
        let peer = &self.setup.peer;
        let session = self.sessions.entry(trace).or_insert_with(|| Session {
            live: Live::new().ok(),
            completed: Vec::new(),
            due: false,
            spans: exporting.then(|| {
                let span_ids = Ids::new();
                Spans::new(
                    ids.trace_id(),
                    peer.clone(),
                    Box::new(move || span_ids.bytes::<8>()),
                )
            }),
        });
        let Some(live) = session.live.as_mut() else {
            return;
        };
        let pushed = live.push(text, at);
        self.tally.exchanges += u64::try_from(pushed.completed.len()).unwrap_or(0);
        session.completed.extend(pushed.completed);
        session.due |= pushed.due;
    }

    /// Judges every session that is due, once.
    fn pass(&mut self) {
        let started = Instant::now();
        let mut printed = String::new();
        let numbered = self.setup.numbered;
        for (trace, session) in &mut self.sessions {
            if !session.due {
                continue;
            }
            session.due = false;
            let Some(live) = session.live.as_mut() else {
                continue;
            };
            let pass = (self.setup.judge)(live);
            self.judged.passes += 1;
            for finding in &pass.new {
                self.tally.count(*trace, finding);
                printed.push_str(&print::finding(finding, numbered.then_some(*trace)));
            }
            self.judged.findings += u64::try_from(pass.new.len()).unwrap_or(0);
            if let (Some(spans), Some(sink)) = (session.spans.as_mut(), &self.setup.spans) {
                for span in spans.pass(&session.completed, &pass.new, SystemTime::now()) {
                    sink.send(span);
                }
            }
            session.completed.clear();
        }
        self.tally.sessions = self.sessions.len();
        printed.push_str(&self.loss_notice());
        self.show(&printed);
        // At most a quarter of a core: wait three times as long as this pass took.
        self.next_pass = Instant::now() + started.elapsed().saturating_mul(3);
    }

    /// Writes `printed` and the tally: on a terminal, the tally line rewritten
    /// in place under the findings; elsewhere, a tally line after new findings.
    fn show(&mut self, printed: &str) {
        let out = &mut self.setup.out;
        if self.setup.terminal {
            let _ = write!(out, "\r\x1b[2K{printed}{}", self.tally.line());
        } else if !printed.is_empty() {
            let _ = writeln!(out, "{printed}{}", self.tally.line());
        }
        let _ = out.flush();
    }

    /// The end: each session's final judgment, its last findings and spans,
    /// and the validator's report.
    fn finish(&mut self) {
        let now = SystemTime::now();
        let numbered = self.setup.numbered;
        let mut text = String::new();
        if self.setup.terminal {
            text.push_str("\r\x1b[2K");
        }
        text.push_str(&self.loss_notice());
        let lossy = self.queue.lost.load(Ordering::Relaxed) > 0;
        for (trace, session) in &mut self.sessions {
            if lossy {
                // The judge's copy has holes: judge what the recorder wrote.
                let name = (self.setup.name)(*trace);
                match judged_from_file(&name) {
                    Ok(judgment) => text.push_str(&print::report(&name, &judgment)),
                    Err(error) => {
                        let _ = writeln!(text, "{} {name} was not judged: {error}", print::PREFIX);
                    }
                }
                continue;
            }
            let Some(live) = session.live.as_mut() else {
                continue;
            };
            let ended = live.finish(now);
            for finding in &ended.new {
                self.tally.count(*trace, finding);
                text.push_str(&print::finding(finding, numbered.then_some(*trace)));
            }
            self.judged.findings += u64::try_from(ended.new.len()).unwrap_or(0);
            for finding in &ended.withdrawn {
                text.push_str(&print::withdrawn(finding, numbered.then_some(*trace)));
            }
            self.judged.withdrawn += u64::try_from(ended.withdrawn.len()).unwrap_or(0);
            if let (Some(spans), Some(sink)) = (session.spans.as_mut(), &self.setup.spans) {
                session.completed.extend(ended.unanswered);
                for span in spans.finish(&session.completed, &ended.new, now) {
                    sink.send(span);
                }
                self.judged.unattached += spans.unattached();
            }
            let name = (self.setup.name)(*trace);
            match &ended.judgment {
                Ok(judgment) => text.push_str(&print::report(&name, judgment)),
                Err(error) => {
                    let _ = writeln!(text, "{} {name} was not judged: {error}", print::PREFIX);
                }
            }
        }
        self.tally.sessions = self.sessions.len();
        text.push_str(&self.tally.line());
        text.push('\n');
        let _ = self.setup.out.write_all(text.as_bytes());
        let _ = self.setup.out.flush();
        // Dropping the sink closes the export queue: the exporter sends what
        // it holds and stops.
        self.setup.spans = None;
    }
}

/// The trace file `name`, judged as `mcp-trace-validator validate` would.
fn judged_from_file(name: &str) -> Result<judge::Judgment, String> {
    let document =
        std::fs::read_to_string(name).map_err(|error| format!("cannot read {name}: {error}"))?;
    judge::judge(&document).map_err(|error| error.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
