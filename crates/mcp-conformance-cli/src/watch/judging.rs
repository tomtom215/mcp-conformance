// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The judging thread: off the session's path, always.
//!
//! The recorder shows each line to a [`Tap`], which copies it onto an
//! unbounded channel and returns — the only work `watch` adds to forwarding
//! a message. A thread of its own takes lines off the channel, pairs them into
//! exchanges, and judges. If judging falls behind, lines wait in the channel
//! (each line is held once more, as the judge holds the whole trace anyway)
//! and the next pass covers all of them: the findings lag, the session never
//! does.
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
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Instant, SystemTime};

use mcp_conformance_cli::live::{Exchange, Live, Pass};
use mcp_conformance_cli::otlp::{Peer, Spans};
use mcp_trace_capture::Observer;

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

/// The recorder's observer: a channel, nothing more.
#[derive(Debug)]
pub(super) struct Tap(pub(super) Sender<Note>);

impl Observer for Tap {
    fn observe(&self, trace: u64, line: &[u8]) {
        let _ = self.0.send(Note::Line {
            trace,
            at: SystemTime::now(),
            line: line.to_vec(),
        });
    }
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
    let (notes, received) = mpsc::channel();
    let thread = std::thread::spawn(move || Thread::new(setup).run(&received));
    (Tap(notes), thread)
}

struct Thread {
    setup: Setup,
    ids: Ids,
    sessions: BTreeMap<u64, Session>,
    tally: Tally,
    judged: Judged,
    next_pass: Instant,
}

impl Thread {
    fn new(setup: Setup) -> Self {
        Self {
            setup,
            ids: Ids::new(),
            sessions: BTreeMap::new(),
            tally: Tally::default(),
            judged: Judged::default(),
            next_pass: Instant::now(),
        }
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
                    Note::Line { trace, at, line } => self.take(trace, at, &line),
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
                self.tally.count(finding);
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
        for (trace, session) in &mut self.sessions {
            let Some(live) = session.live.as_mut() else {
                continue;
            };
            let ended = live.finish(now);
            for finding in &ended.new {
                self.tally.count(finding);
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
