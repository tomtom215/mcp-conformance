// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Who a finding is held against: the clause's actor, else the sender of the
//! message at its `seq`, else nobody.

use mcp_conformance_core::requirement::{Actor, Registry};
use mcp_conformance_core::trace::Direction;

use super::super::{attribute, sender_at, validate};
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};
use crate::report::{Finding, Party};

/// A session whose client omits `notifications/initialized` (LIFE-003, a
/// client clause whose finding points at the *server's* result), calls
/// `logging/setLevel` on a server that declared no `logging` (LIFE-009, a
/// clause binding both parties, broken by the client), and is asked to sample
/// by a server although it declared no `sampling` (LIFE-009 again, broken by
/// the server).
const MIXED: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"logging/setLevel","params":{"level":"info"}}}
{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{}}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":"s1","method":"sampling/createMessage","params":{"messages":[],"maxTokens":1}}}
{"seq":5,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-close"}"#;

fn findings_of(id: &str) -> Vec<Finding> {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let events = parse_trace(MIXED, &Limits::default()).unwrap();
    let report = validate(&registry, &events);
    report
        .requirements
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
        .findings
        .clone()
}

#[test]
fn a_one_party_clause_is_that_partys_whatever_event_the_finding_names() {
    // LIFE-003's finding points at seq 1, the server's result; the omission
    // it reports is the client's, and the registry says so.
    let life_003 = findings_of("LIFE-003");
    assert_eq!(life_003.len(), 1, "{life_003:?}");
    assert_eq!(life_003[0].seq, Some(1));
    assert_eq!(life_003[0].party, Some(Party::Client));
}

#[test]
fn a_both_parties_clause_is_held_against_the_sender_of_the_message() {
    let life_009 = findings_of("LIFE-009");
    let held: Vec<(Option<u64>, Option<Party>)> = life_009
        .iter()
        .map(|finding| (finding.seq, finding.party))
        .collect();
    assert_eq!(
        held,
        [
            (Some(2), Some(Party::Client)),
            (Some(4), Some(Party::Server))
        ],
        "{life_009:?}"
    );
    assert!(life_009[0].detail.contains("logging/setLevel"));
    assert!(life_009[1].detail.contains("sampling/createMessage"));
}

#[test]
fn every_row_carries_the_registrys_actor() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let events = parse_trace(MIXED, &Limits::default()).unwrap();
    let report = validate(&registry, &events);
    for (row, requirement) in report.requirements.iter().zip(registry.requirements()) {
        assert_eq!(row.id, requirement.id.as_str());
        assert_eq!(row.actor, Some(requirement.actor), "{}", row.id);
    }
    let life_009 = report
        .requirements
        .iter()
        .find(|row| row.id == "LIFE-009")
        .unwrap();
    assert_eq!(life_009.actor, Some(Actor::Both));
}

#[test]
fn nobody_is_blamed_without_a_message_to_blame() {
    let events = parse_trace(MIXED, &Limits::default()).unwrap();
    let context = TraceContext::new(&events);
    // The message events name their senders.
    assert_eq!(sender_at(&context, 0), Some(Party::Client));
    assert_eq!(sender_at(&context, 1), Some(Party::Server));
    // A lifecycle event has a direction and is still not a message.
    assert_eq!(sender_at(&context, 5), None);
    // A seq the trace does not hold.
    assert_eq!(sender_at(&context, 99), None);

    let finding = |seq| Finding {
        check: "x".to_owned(),
        seq,
        party: None,
        detail: String::new(),
    };
    // Both parties, no seq: nobody.
    let mut findings = vec![finding(None), finding(Some(5)), finding(Some(2))];
    attribute(&mut findings, Actor::Both, &context);
    let held: Vec<Option<Party>> = findings.iter().map(|finding| finding.party).collect();
    assert_eq!(held, [None, None, Some(Party::Client)]);
    // One party: that party, seq or no seq.
    let mut findings = vec![finding(None), finding(Some(1))];
    attribute(&mut findings, Actor::Server, &context);
    assert!(
        findings
            .iter()
            .all(|finding| finding.party == Some(Party::Server))
    );
    let mut findings = vec![finding(Some(1))];
    attribute(&mut findings, Actor::Client, &context);
    assert_eq!(findings[0].party, Some(Party::Client));
}

#[test]
fn the_party_helpers_are_exact() {
    assert_eq!(Party::sender(Direction::ClientToServer), Party::Client);
    assert_eq!(Party::sender(Direction::ServerToClient), Party::Server);
    assert_eq!(Party::of_actor(Actor::Client), Some(Party::Client));
    assert_eq!(Party::of_actor(Actor::Server), Some(Party::Server));
    assert_eq!(Party::of_actor(Actor::Both), None);
    assert_eq!(Party::Client.other(), Party::Server);
    assert_eq!(Party::Server.other(), Party::Client);
    assert_eq!(Party::Client.as_str(), "client");
    assert_eq!(Party::Server.to_string(), "server");
    assert_eq!(serde_json::to_value(Party::Server).unwrap(), "server");
}

#[test]
fn the_wire_schema_holds_the_sender_to_it() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let invalid = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s"}}}}"#;
    let events = parse_trace(invalid, &Limits::default()).unwrap();
    let report = validate(&registry, &events);
    let schema = report.wire_schema.unwrap();
    assert_eq!(schema.findings.len(), 1, "{:?}", schema.findings);
    assert_eq!(schema.findings[0].party, Some(Party::Server));
}
