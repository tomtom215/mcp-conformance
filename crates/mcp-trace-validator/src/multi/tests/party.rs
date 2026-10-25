// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A multi-revision report viewed for one party: the verdict each revision
//! gets from the findings that count, and how the report says so.

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::requirement::Actor;

use super::super::*;
use super::revs;
use crate::reader::{Limits, parse_trace};

/// A `2025-11-25` session whose client reuses a request id (BASE-003, held
/// against the client) and whose server answers `initialize` without
/// `serverInfo` (LIFE-010, held against the server; the wire schema too).
const TWO_PARTIES: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#;

#[test]
fn a_party_view_recomputes_each_revisions_verdict_and_says_so() {
    let set = RegistrySet::builtin().unwrap();
    let events = parse_trace(TWO_PARTIES, &Limits::default()).unwrap();
    let mut report = validate_revisions(&set, &revs(), &events).unwrap();
    assert_eq!(report.summaries[0].verdict, Verdict::Fail);
    let base_003 = report
        .requirements
        .iter()
        .find(|row| row.id == "BASE-003")
        .unwrap();
    assert_eq!(base_003.actor, Some(Actor::Both));
    assert_eq!(base_003.findings[0][0].party, Some(Party::Client));

    report.view_party(Party::Client);
    assert_eq!(report.party_view, Some(Party::Client));
    // The client's own BASE-003 still fails its view at 2025-11-25.
    assert_eq!(report.summaries[0].verdict, Verdict::Fail);
    let human = report.render_human();
    assert!(
        human.contains("[client] 2025-11-25 seq 3: request \"ping\" reuses id 1"),
        "{human}"
    );
    assert!(
        human.contains("— verdict fail (client findings only: "),
        "{human}"
    );
    assert!(
        human.contains("overall verdict (client findings only): fail"),
        "{human}"
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["party_view"], "client");
    assert_eq!(json["requirements"][0]["actor"], "both");

    // Viewed for the server, the client's reuse is set aside; LIFE-010 and
    // the schema (the server's) still fail.
    let mut report = validate_revisions(&set, &revs(), &events).unwrap();
    report.view_party(Party::Server);
    assert_eq!(report.summaries[0].verdict, Verdict::Fail);
    assert!(
        report
            .render_human()
            .contains("server findings only: 1 client finding set aside"),
        "{}",
        report.render_human()
    );
}

#[test]
fn a_party_view_keeps_a_revision_with_unsupported_rows_unsupported() {
    let set = RegistrySet::builtin().unwrap();
    let events = parse_trace(TWO_PARTIES, &Limits::default()).unwrap();
    let mut report = validate_revisions(&set, &revs(), &events).unwrap();
    report.summaries[1].totals.unsupported = 1;
    report.view_party(Party::Client);
    assert_eq!(report.summaries[1].verdict, Verdict::Unsupported);
    assert_eq!(report.summaries[0].verdict, Verdict::Fail);
}

#[test]
fn a_failing_wire_schema_fails_the_party_it_is_held_against_alone() {
    // Every clause finding is the client's; the schema's are the server's. The
    // server's view fails on the schema alone, and the client's on its clauses.
    let set = RegistrySet::builtin().unwrap();
    let events = parse_trace(TWO_PARTIES, &Limits::default()).unwrap();
    let mut report = validate_revisions(&set, &revs(), &events).unwrap();
    for row in &mut report.requirements {
        for finding in row.findings.iter_mut().flatten() {
            finding.party = Some(Party::Client);
        }
    }
    let schema = report.summaries[0].wire_schema.as_mut().unwrap();
    assert_eq!(schema.outcome, Outcome::Fail);
    for finding in &mut schema.findings {
        finding.party = Some(Party::Server);
    }
    report.view_party(Party::Server);
    assert_eq!(report.summaries[0].verdict, Verdict::Fail);
    // The schema's findings set aside, nothing is left against the client but
    // its clause findings, which fail it.
    report.view_party(Party::Client);
    assert_eq!(report.summaries[0].verdict, Verdict::Fail);
}

#[test]
fn a_warning_held_against_the_other_party_warns_nobody_else() {
    let set = RegistrySet::builtin().unwrap();
    let events = parse_trace(TWO_PARTIES, &Limits::default()).unwrap();
    let mut report = validate_revisions(&set, &revs(), &events).unwrap();
    let mut held = report
        .requirements
        .iter()
        .find(|row| row.id == "BASE-003")
        .unwrap()
        .findings[0][0]
        .clone();
    held.party = Some(Party::Client);
    // Nothing fails or warns anywhere, except BASE-003's warning against the client.
    for row in &mut report.requirements {
        for outcome in row.outcomes.iter_mut().flatten() {
            *outcome = Outcome::Pass;
        }
        for findings in &mut row.findings {
            findings.clear();
        }
    }
    for summary in &mut report.summaries {
        summary.wire_schema = None;
    }
    let row = report
        .requirements
        .iter_mut()
        .find(|row| row.id == "BASE-003")
        .unwrap();
    row.outcomes[0] = Some(Outcome::Warn);
    row.findings[0] = vec![held];
    report.view_party(Party::Client);
    assert_eq!(report.summaries[0].verdict, Verdict::PassWithWarnings);
    report.view_party(Party::Server);
    assert_eq!(report.summaries[0].verdict, Verdict::Pass);
}

#[test]
fn a_revision_with_nothing_against_the_viewed_party_passes_for_it() {
    // With nothing left against the server at a revision, that revision passes
    // for it while the trace's own verdict is unchanged.
    let set = RegistrySet::builtin().unwrap();
    let events = parse_trace(TWO_PARTIES, &Limits::default()).unwrap();
    let mut only_client = validate_revisions(&set, &revs(), &events).unwrap();
    for row in &mut only_client.requirements {
        for finding in row.findings.iter_mut().flatten() {
            finding.party = Some(Party::Client);
        }
    }
    for summary in &mut only_client.summaries {
        if let Some(schema) = &mut summary.wire_schema {
            for finding in &mut schema.findings {
                finding.party = Some(Party::Client);
            }
        }
    }
    assert_eq!(only_client.verdict(), Verdict::Fail);
    only_client.view_party(Party::Server);
    assert_eq!(only_client.summaries[0].verdict, Verdict::Pass);
    assert_eq!(only_client.verdict(), Verdict::Pass);
    assert!(
        only_client
            .render_human()
            .contains("overall verdict (server findings only): pass"),
    );
}
