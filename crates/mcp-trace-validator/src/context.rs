// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Precomputed per-trace context shared by all checks.
//!
//! Checks must be cheap and independent, so anything every check would otherwise
//! recompute — message classification and the session lifecycle phase at each event —
//! is derived once here, in a single deterministic pass over the events.

use mcp_conformance_core::message::{MessageKind, classify};
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

mod pairing;

pub mod stateless;

/// The `2026-07-28` lifecycle machine under its pre-0.6.0 name.
///
/// Renamed [`stateless`] when the revision it models stopped being a draft. Kept
/// for one minor release, as `docs/plan/04-engineering-standards.md` requires of
/// a deprecation; the items are the same ones.
#[deprecated(since = "0.6.0", note = "renamed `context::stateless`")]
pub mod draft {
    pub use super::stateless::*;
}

pub use pairing::Exchange;

/// The `2025-11-25` session lifecycle phase *before* a given event is processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// No `initialize` request has been observed yet.
    BeforeInitialize,
    /// `initialize` was sent; the server has not yet responded to it.
    AwaitingInitializeResult,
    /// The server answered `initialize` with a result; `notifications/initialized`
    /// has not yet been observed.
    AfterInitializeSuccess,
    /// The server answered `initialize` with an error; the session never became ready.
    AfterInitializeError,
    /// `notifications/initialized` has been observed; normal operation.
    Ready,
}

/// The observed `initialize` exchange, when present.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct InitializeExchange<'a> {
    /// The `initialize` request: its event `seq` and `params` value (if any).
    pub request: Option<(u64, Option<&'a Value>)>,
    /// The successful `initialize` result: its event `seq` and `result` value.
    pub result: Option<(u64, &'a Value)>,
    /// The `seq` of the `notifications/initialized` notification.
    pub initialized: Option<u64>,
}

/// Everything checks need, precomputed once per trace.
#[derive(Debug)]
pub struct TraceContext<'a> {
    events: &'a [TraceEvent],
    kinds: Vec<Option<MessageKind<'a>>>,
    phases: Vec<Phase>,
    pairs: Vec<Option<usize>>,
    init: InitializeExchange<'a>,
    final_phase: Phase,
}

impl<'a> TraceContext<'a> {
    /// Builds the context in one pass over the events.
    ///
    /// # Panics
    ///
    /// When `seq` is not strictly increasing across `events`. The trace
    /// schema requires it, [`reader::parse_trace`] rejects documents that
    /// violate it, and several checks compare `seq` values across events on
    /// the premise that no two events share one — a hand-built slice that
    /// breaks the premise would otherwise be judged silently wrong, not
    /// loudly invalid.
    ///
    /// [`reader::parse_trace`]: crate::reader::parse_trace
    #[must_use]
    pub fn new(events: &'a [TraceEvent]) -> Self {
        if let Some(window) = events.windows(2).find(|w| w[0].seq >= w[1].seq) {
            panic!(
                "trace events must have strictly increasing seq (the reader guarantees \
                 this; hand-built slices must too): seq {} is followed by seq {}",
                window[0].seq, window[1].seq
            );
        }
        let kinds: Vec<Option<MessageKind<'a>>> = events
            .iter()
            .map(|event| event.message_payload().map(classify))
            .collect();

        let mut phases = Vec::with_capacity(events.len());
        let mut tracker = LifecycleTracker::start();
        for (event, kind) in events.iter().zip(&kinds) {
            phases.push(tracker.phase);
            if let Some(kind) = kind {
                tracker.step(event, kind);
            }
        }

        let pairs = pairing::pair_responses(events, &kinds);

        Self {
            events,
            kinds,
            phases,
            pairs,
            init: tracker.init,
            final_phase: tracker.phase,
        }
    }

    /// The underlying events.
    #[must_use]
    pub const fn events(&self) -> &'a [TraceEvent] {
        self.events
    }

    /// Iterates `(event, classification, phase-before-event)` triples for message
    /// events only — the shape almost every check wants.
    pub fn messages(&self) -> impl Iterator<Item = (&'a TraceEvent, &MessageKind<'a>, Phase)> + '_ {
        self.events
            .iter()
            .zip(&self.kinds)
            .zip(&self.phases)
            .filter_map(|((event, kind), phase)| kind.as_ref().map(|kind| (event, kind, *phase)))
    }

    /// The observed `initialize` exchange.
    #[must_use]
    pub const fn initialize(&self) -> &InitializeExchange<'a> {
        &self.init
    }

    /// The server's declared capabilities, from the `initialize` result.
    #[must_use]
    pub fn server_capabilities(&self) -> Option<&'a Value> {
        self.init
            .result
            .and_then(|(_, result)| result.get("capabilities"))
    }

    /// The client's declared capabilities, from the `initialize` request params.
    #[must_use]
    pub fn client_capabilities(&self) -> Option<&'a Value> {
        self.init
            .request
            .and_then(|(_, params)| params?.get("capabilities"))
    }

    /// The lifecycle phase after the entire trace has been processed.
    #[must_use]
    pub const fn final_phase(&self) -> Phase {
        self.final_phase
    }
}

/// The `2025-11-25` lifecycle state machine, folded over message events in order.
struct LifecycleTracker<'a> {
    phase: Phase,
    init: InitializeExchange<'a>,
    initialize_id: Option<&'a Value>,
}

impl<'a> LifecycleTracker<'a> {
    const fn start() -> Self {
        Self {
            phase: Phase::BeforeInitialize,
            init: InitializeExchange {
                request: None,
                result: None,
                initialized: None,
            },
            initialize_id: None,
        }
    }

    fn step(&mut self, event: &'a TraceEvent, kind: &MessageKind<'a>) {
        match (self.phase, event.direction, kind) {
            // A client may retry `initialize` after an error answered it — the
            // spec's own example of an initialize error is "Unsupported protocol
            // version", which a client meets by proposing another. The retry is
            // a new attempt: the exchange record follows it, so the capability
            // gates read the declarations the session actually ran under.
            (
                Phase::BeforeInitialize | Phase::AfterInitializeError,
                Direction::ClientToServer,
                MessageKind::Request { method, id },
            ) if *method == "initialize" => {
                self.initialize_id = Some(id);
                self.init.request = Some((
                    event.seq,
                    event
                        .message_payload()
                        .and_then(|payload| payload.get("params")),
                ));
                self.phase = Phase::AwaitingInitializeResult;
            }
            (
                Phase::AwaitingInitializeResult,
                Direction::ServerToClient,
                MessageKind::Result { id: Some(id) },
            ) if Some(*id) == self.initialize_id => {
                self.init.result = event
                    .message_payload()
                    .and_then(|payload| payload.get("result"))
                    .map(|result| (event.seq, result));
                self.phase = Phase::AfterInitializeSuccess;
            }
            (
                Phase::AwaitingInitializeResult,
                Direction::ServerToClient,
                MessageKind::Error { id: Some(id), .. },
            ) if Some(*id) == self.initialize_id => {
                self.phase = Phase::AfterInitializeError;
            }
            (
                Phase::AfterInitializeSuccess,
                Direction::ClientToServer,
                MessageKind::Notification { method },
            ) if *method == "notifications/initialized" => {
                self.init.initialized = Some(event.seq);
                self.phase = Phase::Ready;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
