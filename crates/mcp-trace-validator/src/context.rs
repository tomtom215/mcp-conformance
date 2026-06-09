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
    #[must_use]
    pub fn new(events: &'a [TraceEvent]) -> Self {
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
            (
                Phase::BeforeInitialize,
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::reader::{Limits, parse_trace};

    fn happy_path() -> Vec<TraceEvent> {
        let doc = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}
{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":4,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#;
        parse_trace(doc, &Limits::default()).unwrap()
    }

    #[test]
    fn tracks_phases_through_initialization() {
        let events = happy_path();
        let context = TraceContext::new(&events);
        let phases: Vec<Phase> = context.messages().map(|(_, _, phase)| phase).collect();
        assert_eq!(
            phases,
            vec![
                Phase::BeforeInitialize,
                Phase::AwaitingInitializeResult,
                Phase::AfterInitializeSuccess,
                Phase::Ready,
            ]
        );
        assert_eq!(context.final_phase(), Phase::Ready);
    }

    #[test]
    fn records_initialize_exchange() {
        let events = happy_path();
        let context = TraceContext::new(&events);
        let init = context.initialize();
        assert_eq!(init.request.unwrap().0, 1);
        assert!(init.request.unwrap().1.is_some());
        assert_eq!(init.result.unwrap().0, 2);
        assert_eq!(init.initialized, Some(3));
    }

    #[test]
    fn initialize_error_blocks_ready() {
        let doc = r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}}
{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Unsupported protocol version"}}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;
        let events = parse_trace(doc, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        // The initialized notification after an error result does not make the
        // session Ready.
        assert_eq!(context.initialize().initialized, None);
        assert_eq!(context.final_phase(), Phase::AfterInitializeError);
    }

    #[test]
    fn empty_trace_has_no_exchange() {
        let context = TraceContext::new(&[]);
        assert!(context.initialize().request.is_none());
        assert_eq!(context.final_phase(), Phase::BeforeInitialize);
        assert_eq!(context.server_capabilities(), None);
        assert_eq!(context.client_capabilities(), None);
    }

    #[test]
    fn capability_accessors_read_their_declaration_surfaces() {
        use serde_json::json;
        let events = happy_path();
        let context = TraceContext::new(&events);
        // happy_path declares empty capability sets on both sides.
        assert_eq!(context.client_capabilities(), Some(&json!({})));
        assert_eq!(context.server_capabilities(), Some(&json!({})));

        // A params-less initialize and an answered-by-error exchange expose nothing.
        let doc = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize"}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"x"}}}"#;
        let events = parse_trace(doc, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        assert_eq!(context.client_capabilities(), None);
        assert_eq!(context.server_capabilities(), None);
    }

    #[test]
    fn responses_with_unrelated_ids_do_not_complete_initialization() {
        // Guard pinning: only the response matching the initialize id may transition
        // the phase; an unrelated result or error must leave it Awaiting.
        for body in [
            r#"{"jsonrpc":"2.0","id":99,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":99,"error":{"code":-32600,"message":"x"}}"#,
        ] {
            let response = format!(
                r#"{{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{body}}}"#
            );
            let doc = format!(
                "{}\n{response}",
                r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}}"#,
            );
            let events = parse_trace(&doc, &Limits::default()).unwrap();
            let context = TraceContext::new(&events);
            assert!(context.initialize().result.is_none(), "{body}");
            assert_eq!(
                context.final_phase(),
                Phase::AwaitingInitializeResult,
                "{body}"
            );
        }
    }

    #[test]
    fn only_the_initialized_notification_makes_the_session_ready() {
        let doc = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/cancelled"}}"#;
        let events = parse_trace(doc, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        assert_eq!(context.initialize().initialized, None);
        assert_eq!(context.final_phase(), Phase::AfterInitializeSuccess);
    }

    /// Property coverage for the lifecycle state machine: arbitrary interleavings of
    /// a small message alphabet must never break the machine's invariants.
    mod state_machine_properties {
        use super::*;
        use proptest::prelude::*;
        use serde_json::json;

        /// The alphabet: plausible and implausible protocol moves, both directions.
        /// Events are built through serde (`TraceEvent` is `#[non_exhaustive]`), which
        /// is also how every real trace arrives.
        fn arbitrary_event(seq: u64, choice: u8, direction_bit: bool) -> TraceEvent {
            let payload = match choice % 7 {
                0 => json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
                1 => json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25"}}),
                2 => json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"x"}}),
                3 => json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                4 => json!({"jsonrpc":"2.0","id":99,"result":{}}),
                5 => json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
                _ => json!({"jsonrpc":"2.0","method":"notifications/cancelled"}),
            };
            let direction = if direction_bit {
                "client-to-server"
            } else {
                "server-to-client"
            };
            serde_json::from_value(json!({
                "seq": seq,
                "direction": direction,
                "transport": "stdio",
                "kind": "message",
                "payload": payload,
            }))
            .unwrap()
        }

        /// Allowed transition edges; anything else is a state-machine defect.
        const fn edge_is_legal(from: Phase, to: Phase) -> bool {
            matches!(
                (from, to),
                (
                    Phase::BeforeInitialize,
                    Phase::BeforeInitialize | Phase::AwaitingInitializeResult
                ) | (
                    Phase::AwaitingInitializeResult,
                    Phase::AwaitingInitializeResult
                        | Phase::AfterInitializeSuccess
                        | Phase::AfterInitializeError
                ) | (
                    Phase::AfterInitializeSuccess,
                    Phase::AfterInitializeSuccess | Phase::Ready
                ) | (Phase::AfterInitializeError, Phase::AfterInitializeError)
                    | (Phase::Ready, Phase::Ready)
            )
        }

        proptest! {
            #[test]
            fn invariants_hold_for_arbitrary_sequences(
                moves in proptest::collection::vec((any::<u8>(), any::<bool>()), 0..32)
            ) {
                let events: Vec<TraceEvent> = moves
                    .iter()
                    .enumerate()
                    .map(|(index, (choice, direction))| {
                        arbitrary_event(index as u64, *choice, *direction)
                    })
                    .collect();
                let context = TraceContext::new(&events);

                // Phase-before sequence only walks legal edges, ending at final_phase.
                let phases: Vec<Phase> =
                    context.messages().map(|(_, _, phase)| phase).collect();
                prop_assert_eq!(phases.len(), events.len());
                for window in phases.windows(2) {
                    prop_assert!(
                        edge_is_legal(window[0], window[1]),
                        "illegal edge {:?} -> {:?}",
                        window[0],
                        window[1]
                    );
                }
                if let Some(last) = phases.last() {
                    prop_assert!(
                        edge_is_legal(*last, context.final_phase()),
                        "illegal final edge {:?} -> {:?}",
                        last,
                        context.final_phase()
                    );
                }

                // Exchange-record implications.
                let init = context.initialize();
                if init.result.is_some() || init.initialized.is_some() {
                    prop_assert!(init.request.is_some());
                }
                if init.initialized.is_some() {
                    prop_assert!(init.result.is_some());
                    prop_assert_eq!(context.final_phase(), Phase::Ready);
                }
                if context.final_phase() == Phase::Ready {
                    prop_assert!(init.initialized.is_some());
                }
            }
        }
    }
}
