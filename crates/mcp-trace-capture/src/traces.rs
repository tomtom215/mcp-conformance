// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which session, and so which trace, each proxied HTTP exchange belongs to.
//!
//! The validator judges a trace as one session, and a proxy outlives the sessions
//! it carries. What the wire offers to tell them apart:
//!
//! - **`Mcp-Session-Id`** (`2025-11-25`). The server assigns it on the response to
//!   `initialize`; the client sends it on every later request. A request naming an
//!   id the server assigned belongs to that id's session. An id the proxy has not
//!   seen (it started mid-session, or the client sent a stale or made-up one) is
//!   not taken on the client's word: every such request goes to one trace kept
//!   for them, until the server answers one with a 2xx. That id is then the
//!   trace's session — the one later id-less requests join — and the next
//!   unseen id starts a trace of its own. An id the server refuses (404) is not
//!   remembered, so a client inventing ids cannot make the proxy open a file, or
//!   keep a map entry, per id.
//! - **`DELETE` with an id, answered 2xx**, ends that session: its id is
//!   forgotten, so a later request naming it is an unseen id's.
//! - **`initialize` without an id** begins a session: it is how every
//!   `2025-11-25` session starts, with or without the server assigning an id.
//! - **Nothing else.** A request with neither belongs to the session begun most
//!   recently. `2026-07-28` has no sessions at all — no `initialize`, no id — so
//!   its traffic is one session for the life of the proxy, as the wire offers no
//!   finer identity.
//!
//! With one trace for everything ([`Traces::one`]) every exchange goes to it, and
//! the recorder's own session count warns at the end if it holds several. With
//! [`Traces::per_session`], each session gets the next [`Numbered`] file, `seq`
//! from 0.
//!
//! This decides only which file an event is written to. Every exchange is
//! forwarded to the one upstream exactly as before: telling sessions apart for the
//! trace is recording, not the routing a gateway does (ADR-0019).

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use mcp_conformance_core::trace::{Direction, LifecycleEvent, TransportKind};

use crate::framing::parse_json;
use crate::numbered::Numbered;
use crate::recorder::{Observer, Recorder, Summary};

/// Routes proxied exchanges to traces, one per session or one for all.
#[derive(Debug)]
pub struct Traces {
    output: Output,
    max_line: usize,
    state: Mutex<State>,
    observer: Option<Arc<dyn Observer>>,
}

#[derive(Debug)]
enum Output {
    One(Arc<Recorder>),
    PerSession(Numbered),
}

#[derive(Debug, Default)]
struct State {
    sessions: Vec<Session>,
    /// The ids the server assigned or accepted, and their sessions.
    by_id: HashMap<String, usize>,
    current: Option<usize>,
    /// The trace for requests naming an id the server has not yet accepted.
    unseen: Option<usize>,
    next_number: u64,
}

#[derive(Debug)]
struct Session {
    recorder: Arc<Recorder>,
    path: Option<PathBuf>,
    id: Option<String>,
}

/// The trace one exchange is recorded in: hand it back to
/// [`Traces::respond`] with the response's session id.
#[derive(Debug, Clone)]
pub struct Route {
    index: usize,
    recorder: Arc<Recorder>,
    /// The request's session id, when the server has not accepted it yet.
    unseen: Option<String>,
}

impl Route {
    /// The recorder for this exchange's session.
    #[must_use]
    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }
}

/// One trace's outcome, from [`Traces::finish`].
#[derive(Debug)]
#[non_exhaustive]
pub struct Finished {
    /// The session's file; `None` for [`Traces::one`]'s recorder, whose path
    /// the caller chose.
    pub path: Option<PathBuf>,
    /// How its recording ended.
    pub summary: Summary,
}

impl Traces {
    /// Every session into `recorder`, warning on stderr when a second begins.
    #[must_use]
    pub fn one(recorder: Arc<Recorder>) -> Self {
        Self::new(Output::One(recorder), 0)
    }

    /// Each session into the next numbered file `numbered` names, lines up to
    /// `max_line` bytes. A file is opened only for the moment of each write, so
    /// a long-lived proxy holds no descriptor per session.
    #[must_use]
    pub fn per_session(numbered: Numbered, max_line: usize) -> Self {
        Self::new(Output::PerSession(numbered), max_line)
    }

    fn new(output: Output, max_line: usize) -> Self {
        Self {
            output,
            max_line,
            state: Mutex::new(State {
                next_number: 1,
                ..State::default()
            }),
            observer: None,
        }
    }

    /// These traces, each session's recorder showing what it writes to
    /// `observer` under the session's number. [`Traces::one`]'s recorder is
    /// the caller's, observed (or not) as the caller made it.
    #[must_use]
    pub fn observed(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// The session of a request carrying `session_id` (its `Mcp-Session-Id`)
    /// and `body` (when it was read whole).
    pub fn route(&self, session_id: Option<&str>, body: Option<&[u8]>) -> Route {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut unseen = None;
        let index = if let Some(id) = session_id {
            let known = state.by_id.get(id).copied();
            known.unwrap_or_else(|| {
                unseen = Some(id.to_owned());
                let pending = state.unseen;
                pending.unwrap_or_else(|| {
                    let index = self.begin(&mut state, false);
                    state.unseen = Some(index);
                    index
                })
            })
        } else if body.is_some_and(is_initialize) {
            self.begin(&mut state, true)
        } else {
            let current = state.current;
            current.unwrap_or_else(|| self.begin(&mut state, true))
        };
        Route {
            index,
            recorder: Arc::clone(&state.sessions[index].recorder),
            unseen,
        }
    }

    /// Notes how the server answered `route`'s request: the session id the
    /// response carried — the one an `initialize` response assigns names that
    /// session from then on — and whether it `succeeded` (a 2xx), which
    /// accepts an unseen id the request named: its trace becomes that id's
    /// session, and the current one.
    pub fn respond(&self, route: &Route, session_id: Option<&str>, succeeded: bool) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(accepted) = route.unseen.as_deref().filter(|_| succeeded)
            && !state.by_id.contains_key(accepted)
        {
            if state.unseen == Some(route.index) {
                state.unseen = None;
            }
            state.current = Some(route.index);
            state.by_id.insert(accepted.to_owned(), route.index);
            state.sessions[route.index]
                .id
                .get_or_insert_with(|| accepted.to_owned());
        }
        let Some(id) = session_id else { return };
        if state.sessions[route.index].id.is_none() && !state.by_id.contains_key(id) {
            state.sessions[route.index].id = Some(id.to_owned());
            state.by_id.insert(id.to_owned(), route.index);
        }
    }

    /// Forgets `session_id`: the server accepted the client's `DELETE` of that
    /// session. A later request naming it is routed as an unseen id's.
    pub fn end(&self, session_id: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.by_id.remove(session_id);
    }

    /// Whether each session gets a trace of its own.
    #[must_use]
    pub const fn is_per_session(&self) -> bool {
        matches!(self.output, Output::PerSession(_))
    }

    /// Closes every trace with `event`: the proxy is stopping, and a session
    /// that begins after this is not recorded.
    pub fn close_all(&self, event: LifecycleEvent) {
        // Collected first, so no trace is written with the router's lock held.
        let recorders: Vec<Arc<Recorder>> = match &self.output {
            Output::One(recorder) => vec![Arc::clone(recorder)],
            Output::PerSession(_) => self
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .sessions
                .iter()
                .map(|session| Arc::clone(&session.recorder))
                .collect(),
        };
        for recorder in recorders {
            let _ = recorder.close(
                Direction::ClientToServer,
                TransportKind::StreamableHttp,
                event,
            );
        }
    }

    /// Flushes every trace and reports how each ended, in session order.
    pub fn finish(&self) -> Vec<Finished> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &self.output {
            Output::One(recorder) => vec![Finished {
                path: None,
                summary: recorder.finish(),
            }],
            Output::PerSession(_) => state
                .sessions
                .iter()
                .map(|session| Finished {
                    path: session.path.clone(),
                    summary: session.recorder.finish(),
                })
                .collect(),
        }
    }

    /// Starts a session, and makes it the current one if `current`.
    ///
    /// Creating its file (a directory listing and a create) happens here, on
    /// the async handler and under the router's lock, as every write to a trace
    /// happens on the handler that records it: the recording is synchronous by
    /// design, so a message is written before its bytes are relayed. It runs
    /// once per session a client begins or the server accepts — never per id
    /// a client invents.
    fn begin(&self, state: &mut State, current: bool) -> usize {
        let (recorder, path) = match &self.output {
            Output::One(recorder) => (Arc::clone(recorder), None),
            Output::PerSession(numbered) => match numbered.create_next(state.next_number) {
                Ok((number, path, _file)) => {
                    state.next_number = number + 1;
                    eprintln!(
                        "mcp-trace-capture: session {number} recording to {}",
                        path.display()
                    );
                    let sink = Appender(path.clone());
                    let recorder = Recorder::with_max_line(sink, self.max_line);
                    let recorder = match &self.observer {
                        Some(observer) => recorder.observed(Arc::clone(observer), number),
                        None => recorder,
                    };
                    (Arc::new(recorder), Some(path))
                }
                // The recorder reports the failure on the session's first event,
                // and `finish` reports the trace as incomplete.
                Err(error) => (
                    Arc::new(Recorder::with_max_line(Unwritable(error), self.max_line)),
                    None,
                ),
            },
        };
        state.sessions.push(Session {
            recorder,
            path,
            id: None,
        });
        let index = state.sessions.len() - 1;
        if current {
            state.current = Some(index);
        }
        index
    }
}

/// Whether `body` is an `initialize` request, alone or in a batch.
fn is_initialize(body: &[u8]) -> bool {
    let initialize = |message: &serde_json::Value| {
        message.get("method").and_then(serde_json::Value::as_str) == Some("initialize")
            && message.get("id").is_some()
    };
    match parse_json(body) {
        Some(serde_json::Value::Array(batch)) => batch.iter().any(initialize),
        Some(message) => initialize(&message),
        None => false,
    }
}

/// Appends each write to a file it opens only for that write.
struct Appender(PathBuf);

impl Write for Appender {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut file = std::fs::OpenOptions::new().append(true).open(&self.0)?;
        file.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A sink whose file could not be created: every write fails with why.
struct Unwritable(io::Error);

impl Write for Unwritable {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(self.0.kind(), self.0.to_string()))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
