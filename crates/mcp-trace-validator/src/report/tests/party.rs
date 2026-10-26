// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A report viewed for one party: what counts, what the verdict becomes, and
//! how the human and JSON forms say so.

use super::super::*;
use super::{row, sample};

/// [`sample`] with its one failing row held against the client, a warning
/// row held against the server, and a failed wire schema whose one finding is
/// held against nobody.
fn mixed() -> Report {
    let mut report = sample();
    let mut warned = row("TOOL-006", "SHOULD", Outcome::Warn);
    warned.findings = vec![Finding {
        check: "tools.name-charset".to_owned(),
        seq: Some(5),
        party: Some(Party::Server),
        detail: "tool name has a space".to_owned(),
    }];
    report.requirements.push(warned);
    report.totals.warn = 1;
    report.wire_schema = Some(WireSchemaReport {
        outcome: Outcome::Fail,
        messages: 2,
        findings: vec![Finding {
            check: "wire-schema".to_owned(),
            seq: None,
            party: None,
            detail: "not valid".to_owned(),
        }],
        schema: "https://example.test/schema.json".to_owned(),
        source: ClauseSource {
            section: "basic#schema".to_owned(),
            quote: "the source of truth".to_owned(),
            url: "https://example.test/basic#schema".to_owned(),
        },
    });
    report
}

#[test]
fn a_finding_counts_for_its_party_and_an_unattributed_one_for_both() {
    let held = |party| Finding {
        check: "x".to_owned(),
        seq: None,
        party,
        detail: String::new(),
    };
    assert!(held(Some(Party::Client)).counts_for(Party::Client));
    assert!(!held(Some(Party::Client)).counts_for(Party::Server));
    assert!(held(None).counts_for(Party::Client));
    assert!(held(None).counts_for(Party::Server));
}

#[test]
fn the_view_recomputes_the_verdict_from_the_findings_that_count() {
    let report = mixed();
    assert_eq!(report.verdict(), Verdict::Fail);
    // The unattributed schema finding counts for everyone, so both views fail.
    assert_eq!(report.verdict_for(Party::Client), Verdict::Fail);
    assert_eq!(report.verdict_for(Party::Server), Verdict::Fail);

    let mut schema_attributed = report;
    schema_attributed.wire_schema.as_mut().unwrap().findings[0].party = Some(Party::Client);
    // Now the server's view sees only its warning.
    assert_eq!(
        schema_attributed.verdict_for(Party::Server),
        Verdict::PassWithWarnings
    );
    assert_eq!(schema_attributed.verdict_for(Party::Client), Verdict::Fail);

    // A warning held against the client warns nobody else.
    let mut client_warning = schema_attributed.clone();
    client_warning.requirements.last_mut().unwrap().findings[0].party = Some(Party::Client);
    assert_eq!(client_warning.verdict_for(Party::Server), Verdict::Pass);

    let mut nothing_for_server = schema_attributed.clone();
    nothing_for_server.requirements.pop();
    nothing_for_server.totals.warn = 0;
    assert_eq!(nothing_for_server.verdict_for(Party::Server), Verdict::Pass);
    assert_eq!(
        nothing_for_server.verdict(),
        Verdict::Fail,
        "the trace's own verdict is unchanged"
    );

    // Unsupported outranks everything, whoever the findings are held against.
    let mut unsupported = nothing_for_server.clone();
    unsupported.totals.unsupported = 1;
    assert_eq!(unsupported.verdict_for(Party::Server), Verdict::Unsupported);
}

#[test]
fn the_effective_verdict_is_the_views_when_one_is_set() {
    let mut report = mixed();
    report.wire_schema.as_mut().unwrap().findings[0].party = Some(Party::Client);
    assert_eq!(report.effective_verdict(), Verdict::Fail);
    report.party_view = Some(Party::Server);
    assert_eq!(report.effective_verdict(), Verdict::PassWithWarnings);
    assert_eq!(report.verdict(), Verdict::Fail);
}

#[test]
fn the_tally_counts_each_finding_once_and_describes_itself() {
    let report = mixed();
    let server = report.party_tally(Party::Server);
    assert_eq!(
        (server.counted, server.set_aside, server.unattributed),
        (1, 1, 1)
    );
    assert_eq!(
        server.describe(Party::Server),
        "1 client finding set aside, 1 unattributed finding counted"
    );
    let client = report.party_tally(Party::Client);
    assert_eq!(
        (client.counted, client.set_aside, client.unattributed),
        (1, 1, 1)
    );
    assert_eq!(
        client.describe(Party::Client),
        "1 server finding set aside, 1 unattributed finding counted"
    );
    let several = PartyTally {
        counted: 0,
        set_aside: 2,
        unattributed: 3,
    };
    assert_eq!(
        several.describe(Party::Client),
        "2 server findings set aside, 3 unattributed findings counted"
    );
    assert_eq!(PartyTally::default().describe(Party::Client), "");
}

#[test]
fn the_human_report_tags_each_finding_with_its_party() {
    let report = mixed();
    let text = report.render_human();
    assert!(
        text.contains("        [client] seq 3: first message is"),
        "{text}"
    );
    assert!(
        text.contains("        [server] seq 5: tool name has a space"),
        "{text}"
    );
    // Held against nobody, no view: no tag.
    assert!(text.contains("        not valid\n"), "{text}");
    assert!(text.contains("\nverdict: fail\n"), "{text}");
    assert!(!text.contains("findings only"), "{text}");
}

#[test]
fn the_human_report_says_which_view_its_verdict_was_reached_under() {
    let mut report = mixed();
    report.party_view = Some(Party::Server);
    let text = report.render_human();
    // The unattributed finding is labelled as counted-not-attributed.
    assert!(
        text.contains("        [unattributed] not valid\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "\nverdict (server findings only): fail — 1 client finding set aside, \
             1 unattributed finding counted\n"
        ),
        "{text}"
    );
    // The totals still count every finding.
    assert!(text.contains("totals: 1 pass, 1 fail, 1 warn,"), "{text}");

    // Nothing set aside and nothing unattributed: the bare line.
    let mut own = sample();
    own.party_view = Some(Party::Client);
    let text = own.render_human();
    assert!(
        text.contains("\nverdict (client findings only): fail\n"),
        "{text}"
    );
}

#[test]
fn the_json_carries_the_party_per_finding_the_actor_per_row_and_the_view() {
    let mut report = mixed();
    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("party_view").is_none(), "{json}");
    assert_eq!(json["verdict"], "fail");
    assert_eq!(json["requirements"][1]["actor"], "server");
    assert_eq!(json["requirements"][1]["findings"][0]["party"], "client");
    assert!(
        json["wire_schema"]["findings"][0].get("party").is_none(),
        "{json}"
    );
    // The view, and the verdict it governs, in the member order a reader meets.
    report.party_view = Some(Party::Server);
    report.wire_schema.as_mut().unwrap().findings[0].party = Some(Party::Client);
    let text = serde_json::to_string(&report).unwrap();
    assert!(
        text.contains(r#""party_view":"server","verdict":"pass-with-warnings""#),
        "{text}"
    );
    // And it round-trips.
    let back: Report = serde_json::from_str(&text).unwrap();
    assert_eq!(back, report);
}
