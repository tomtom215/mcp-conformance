// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the trace context and its lifecycle tracker.

#![allow(clippy::unwrap_used)]

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
#[should_panic(expected = "strictly increasing seq")]
fn duplicate_seq_is_a_loud_contract_violation() {
    // Checks compare seq values across events assuming uniqueness (e.g.
    // session_id_echoed's cutoff); a hand-built slice with duplicates
    // must fail at the boundary, not be judged silently wrong. The
    // mutants exclusion for `<` vs `<=` in session_id_echoed rests on
    // exactly this enforcement.
    use mcp_conformance_core::trace::{Direction, EventBody, TransportKind};
    let duplicate = vec![
        TraceEvent::new(
            7,
            Direction::ClientToServer,
            TransportKind::Stdio,
            EventBody::Message {
                payload: serde_json::json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
            },
        ),
        TraceEvent::new(
            7,
            Direction::ServerToClient,
            TransportKind::Stdio,
            EventBody::Message {
                payload: serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}}),
            },
        ),
    ];
    let _ = TraceContext::new(&duplicate);
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
fn a_retried_initialize_after_an_error_reaches_ready() {
    let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Unsupported protocol version","data":{"supported":["2025-11-25"]}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{"sampling":{}},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":4,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;
    let events = parse_trace(trace, &Limits::default()).unwrap();
    let context = TraceContext::new(&events);
    assert_eq!(context.final_phase(), Phase::Ready);
    assert_eq!(
        context.initialize().request.unwrap().0,
        2,
        "the retry is the exchange"
    );
    assert!(
        context
            .server_capabilities()
            .is_some_and(|caps| caps.get("tools").is_some())
    );
    assert!(
        context
            .client_capabilities()
            .is_some_and(|caps| caps.get("sampling").is_some())
    );
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
            ) | (
                Phase::AfterInitializeError,
                Phase::AfterInitializeError | Phase::AwaitingInitializeResult
            ) | (Phase::Ready, Phase::Ready)
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
