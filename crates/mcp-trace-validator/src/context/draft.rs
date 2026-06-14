// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The stateless `2026-07-28` lifecycle variant (SEP-2575), behind `draft-2026-07-28`.
//!
//! The `2026-07-28` draft removes the `initialize`/`initialized` handshake (register 1.3,
//! 1.5a): a session is **operational from its first message** — there is no
//! `BeforeInitialize`/`Ready` progression to gate on, which is the defining contrast with
//! the [`2025-11-25` machine](super::Phase). Each request instead carries its protocol
//! context (`protocolVersion`, `clientInfo`, `clientCapabilities`) in `_meta`, and the one
//! handshake-like exchange that remains is the *optional* `server/discover` probe by which
//! a client may read the server's protocol versions, capabilities, and identity.
//!
//! This module models that lifecycle as a second state-machine variant *alongside* — not
//! replacing — the stateful one ([02-architecture.md](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/02-architecture.md)
//! §Protocol-revision strategy). It is intentionally scoped to the **lifecycle** — the
//! phase model and the `server/discover` exchange; per-request `_meta` validation, the
//! removed-method prohibitions (`ping`, `logging/setLevel`,
//! `notifications/roots/list_changed`), and the `UnsupportedProtocolVersionError` rule are
//! registry clauses and checks (roadmap M2.5 line 2), which land with the final spec text.
//!
//! **Draft-tracking:** the shape here follows the SEPs catalogued in register 1.5a–1.5b
//! and must be reconciled against the final `2026-07-28` text when it ships; the gate
//! keeps it off the default build until then.

use mcp_conformance_core::message::{MessageKind, classify};
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

/// The stateless `2026-07-28` lifecycle phase *before* a given event is processed.
///
/// There is no handshake to complete, so the steady state is [`Active`](Self::Active)
/// from the very first event; the only departure is the brief window while a
/// `server/discover` probe is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DraftPhase {
    /// Operational. Requests may flow immediately — the stateless session has no
    /// initialize handshake to complete first (SEP-2575).
    Active,
    /// A `server/discover` request is in flight; its response has not yet been observed.
    AwaitingDiscoverResult,
}

/// The observed `server/discover` exchange — the optional stateless capability/identity
/// probe — when present. A session is valid with no discovery at all.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct DiscoverExchange<'a> {
    /// The `server/discover` request: its event `seq` and `params` value (if any).
    pub request: Option<(u64, Option<&'a Value>)>,
    /// The successful `server/discover` result: its event `seq` and `result` value.
    pub result: Option<(u64, &'a Value)>,
    /// The `seq` of an error response to the `server/discover` request.
    pub error: Option<u64>,
}

/// The stateless lifecycle, folded over a trace's message events in order.
///
/// ```
/// use mcp_trace_validator::context::draft::{DraftLifecycle, DraftPhase};
/// use mcp_conformance_core::trace::TraceEvent;
///
/// // A stateless session: the first message is an ordinary request, with no handshake.
/// let events: Vec<TraceEvent> = serde_json::from_str::<Vec<_>>(r#"[
///     {"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message",
///      "payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}
/// ]"#).unwrap();
///
/// let lifecycle = DraftLifecycle::new(&events);
/// // Operational immediately — no `initialize` required (contrast `2025-11-25`).
/// assert_eq!(lifecycle.phases()[0], DraftPhase::Active);
/// assert_eq!(lifecycle.final_phase(), DraftPhase::Active);
/// assert!(lifecycle.discover().request.is_none());
/// ```
#[derive(Debug)]
pub struct DraftLifecycle<'a> {
    phases: Vec<DraftPhase>,
    discover: DiscoverExchange<'a>,
    final_phase: DraftPhase,
}

impl<'a> DraftLifecycle<'a> {
    /// Folds the stateless lifecycle over `events` in one pass, recording the phase
    /// before each event and the `server/discover` exchange.
    #[must_use]
    pub fn new(events: &'a [TraceEvent]) -> Self {
        let mut phases = Vec::with_capacity(events.len());
        let mut tracker = DraftTracker::start();
        for event in events {
            phases.push(tracker.phase);
            if let Some(kind) = event.message_payload().map(classify) {
                tracker.step(event, &kind);
            }
        }
        Self {
            phases,
            discover: tracker.discover,
            final_phase: tracker.phase,
        }
    }

    /// The phase *before* each event, in trace order (one entry per event).
    #[must_use]
    pub fn phases(&self) -> &[DraftPhase] {
        &self.phases
    }

    /// The lifecycle phase after the entire trace has been processed.
    #[must_use]
    pub const fn final_phase(&self) -> DraftPhase {
        self.final_phase
    }

    /// The observed `server/discover` exchange.
    #[must_use]
    pub const fn discover(&self) -> &DiscoverExchange<'a> {
        &self.discover
    }

    /// The server's declared capabilities, from the `server/discover` result — the
    /// stateless analogue of the `initialize` result's capabilities. `None` when no
    /// discovery completed (the client capability surface lives in each request's `_meta`
    /// in this revision, which is a per-request concern, not a lifecycle one).
    #[must_use]
    pub fn server_capabilities(&self) -> Option<&'a Value> {
        self.discover
            .result
            .and_then(|(_, result)| result.get("capabilities"))
    }
}

/// The folding state machine. One-shot discovery: a `server/discover` is recorded only
/// while no discovery has begun, so the exchange fields are set at most once and the
/// phase is [`AwaitingDiscoverResult`](DraftPhase::AwaitingDiscoverResult) exactly between
/// a recorded request and its matching response.
struct DraftTracker<'a> {
    phase: DraftPhase,
    discover: DiscoverExchange<'a>,
    discover_id: Option<&'a Value>,
}

impl<'a> DraftTracker<'a> {
    const fn start() -> Self {
        Self {
            phase: DraftPhase::Active,
            discover: DiscoverExchange {
                request: None,
                result: None,
                error: None,
            },
            discover_id: None,
        }
    }

    fn step(&mut self, event: &'a TraceEvent, kind: &MessageKind<'a>) {
        match (self.phase, event.direction, kind) {
            (
                DraftPhase::Active,
                Direction::ClientToServer,
                MessageKind::Request { method, id },
            ) if *method == "server/discover" && self.discover.request.is_none() => {
                self.discover_id = Some(id);
                self.discover.request = Some((
                    event.seq,
                    event
                        .message_payload()
                        .and_then(|payload| payload.get("params")),
                ));
                self.phase = DraftPhase::AwaitingDiscoverResult;
            }
            (
                DraftPhase::AwaitingDiscoverResult,
                Direction::ServerToClient,
                MessageKind::Result { id: Some(id) },
            ) if Some(*id) == self.discover_id => {
                self.discover.result = event
                    .message_payload()
                    .and_then(|payload| payload.get("result"))
                    .map(|result| (event.seq, result));
                self.phase = DraftPhase::Active;
            }
            (
                DraftPhase::AwaitingDiscoverResult,
                Direction::ServerToClient,
                MessageKind::Error { id: Some(id), .. },
            ) if Some(*id) == self.discover_id => {
                self.discover.error = Some(event.seq);
                self.phase = DraftPhase::Active;
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

    fn events(doc: &str) -> Vec<TraceEvent> {
        parse_trace(doc, &Limits::default()).unwrap()
    }

    #[test]
    fn operational_from_the_first_message_without_a_handshake() {
        // The defining stateless property: a non-discover request as the very first
        // message is not gated — the session is Active throughout. (Under `2025-11-25`
        // this same trace is a LIFE-001 violation.)
        let trace = events(
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}}"#,
        );
        let lifecycle = DraftLifecycle::new(&trace);
        assert_eq!(lifecycle.phases(), [DraftPhase::Active, DraftPhase::Active]);
        assert_eq!(lifecycle.final_phase(), DraftPhase::Active);
        assert!(lifecycle.discover().request.is_none());
        assert_eq!(lifecycle.server_capabilities(), None);
    }

    #[test]
    fn discover_request_then_result_records_capabilities_and_returns_to_active() {
        let trace = events(
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"x":1}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}}"#,
        );
        let lifecycle = DraftLifecycle::new(&trace);
        // Active before the request, AwaitingDiscoverResult before the response.
        assert_eq!(
            lifecycle.phases(),
            [DraftPhase::Active, DraftPhase::AwaitingDiscoverResult]
        );
        // The response returns the session to Active and records the exchange.
        assert_eq!(lifecycle.final_phase(), DraftPhase::Active);
        assert_eq!(lifecycle.discover().request.unwrap().0, 0);
        assert!(lifecycle.discover().request.unwrap().1.is_some());
        assert_eq!(lifecycle.discover().result.unwrap().0, 1);
        assert!(lifecycle.discover().error.is_none());
        assert_eq!(
            lifecycle.server_capabilities(),
            Some(&serde_json::json!({"tools": {}}))
        );
    }

    #[test]
    fn discover_error_is_an_error_edge_back_to_active() {
        let trace = events(
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":7,"method":"server/discover"}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":7,"error":{"code":-32601,"message":"no discover"}}}"#,
        );
        let lifecycle = DraftLifecycle::new(&trace);
        assert_eq!(lifecycle.final_phase(), DraftPhase::Active);
        assert_eq!(lifecycle.discover().error, Some(1));
        assert!(lifecycle.discover().result.is_none());
        assert_eq!(lifecycle.server_capabilities(), None);
    }

    #[test]
    fn a_response_with_an_unrelated_id_does_not_complete_discovery() {
        // Only the response matching the discover request id may transition back; an
        // unrelated result or error must leave the session awaiting.
        for body in [
            r#"{"jsonrpc":"2.0","id":99,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":99,"error":{"code":-32600,"message":"x"}}"#,
        ] {
            let response = format!(
                r#"{{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{body}}}"#
            );
            let doc = format!(
                "{}\n{response}",
                r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover"}}"#,
            );
            let trace = events(&doc);
            let lifecycle = DraftLifecycle::new(&trace);
            assert_eq!(
                lifecycle.final_phase(),
                DraftPhase::AwaitingDiscoverResult,
                "{body}"
            );
            assert!(lifecycle.discover().result.is_none(), "{body}");
            assert!(lifecycle.discover().error.is_none(), "{body}");
        }
    }

    #[test]
    fn removed_handshake_methods_are_not_lifecycle_transitions() {
        // `initialize` and `notifications/initialized` were removed in the stateless
        // rework; the lifecycle simply does not act on them (they stay non-events here —
        // flagging them is a registry/check concern, roadmap M2.5 line 2).
        let trace = events(
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
        );
        let lifecycle = DraftLifecycle::new(&trace);
        assert!(lifecycle.phases().iter().all(|p| *p == DraftPhase::Active));
        assert_eq!(lifecycle.final_phase(), DraftPhase::Active);
        assert!(lifecycle.discover().request.is_none());
    }

    #[test]
    fn empty_trace_is_active_with_no_discovery() {
        let lifecycle = DraftLifecycle::new(&[]);
        assert!(lifecycle.phases().is_empty());
        assert_eq!(lifecycle.final_phase(), DraftPhase::Active);
        assert!(lifecycle.discover().request.is_none());
        assert_eq!(lifecycle.server_capabilities(), None);
    }

    #[test]
    fn discovery_is_one_shot_a_second_request_while_active_is_ignored() {
        // After a completed discovery the session is Active; a further `server/discover`
        // is not re-recorded (the realistic single-probe model, SEP-2575).
        let trace = events(
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover"}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"server/discover"}}"#,
        );
        let lifecycle = DraftLifecycle::new(&trace);
        // The first discovery is the one recorded; the second leaves us Active.
        assert_eq!(lifecycle.final_phase(), DraftPhase::Active);
        assert_eq!(lifecycle.discover().request.unwrap().0, 0);
        assert_eq!(lifecycle.discover().result.unwrap().0, 1);
    }

    /// Property coverage: arbitrary interleavings of a small message alphabet must never
    /// break the stateless machine's invariants.
    mod properties {
        use super::*;
        use proptest::prelude::*;
        use serde_json::json;

        fn arbitrary_event(seq: u64, choice: u8, direction_bit: bool) -> TraceEvent {
            let payload = match choice % 6 {
                0 => json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{}}),
                1 => json!({"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}),
                2 => json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}}),
                3 => json!({"jsonrpc":"2.0","id":99,"result":{}}),
                4 => json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
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
                let lifecycle = DraftLifecycle::new(&events);

                // One phase-before per event, and a stateless session starts Active.
                prop_assert_eq!(lifecycle.phases().len(), events.len());
                if let Some(first) = lifecycle.phases().first() {
                    prop_assert_eq!(*first, DraftPhase::Active);
                }

                let discover = lifecycle.discover();
                // Awaiting iff a discovery was requested whose response has not arrived.
                let outstanding =
                    discover.request.is_some() && discover.result.is_none() && discover.error.is_none();
                prop_assert_eq!(lifecycle.final_phase() == DraftPhase::AwaitingDiscoverResult, outstanding);

                // A response is only ever recorded against a request, and never both.
                if discover.result.is_some() || discover.error.is_some() {
                    prop_assert!(discover.request.is_some());
                }
                prop_assert!(!(discover.result.is_some() && discover.error.is_some()));

                // Entering AwaitingDiscoverResult requires a client `server/discover`
                // request at that step — the transition is never spurious.
                for (index, pair) in lifecycle.phases().windows(2).enumerate() {
                    if pair[0] == DraftPhase::Active && pair[1] == DraftPhase::AwaitingDiscoverResult {
                        let event = &events[index];
                        prop_assert_eq!(event.direction, Direction::ClientToServer);
                        let kind = event.message_payload().map(classify);
                        let is_discover_request = matches!(
                            kind,
                            Some(MessageKind::Request { method, .. }) if method == "server/discover"
                        );
                        prop_assert!(is_discover_request);
                    }
                }
            }
        }
    }
}
