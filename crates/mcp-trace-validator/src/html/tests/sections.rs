// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The page's sections in detail: heading levels under every nesting, the
//! extension section's empty state, the wire-schema line, and the timeline's
//! switch and truncation note.

use super::{SESSION, judged, page};
use crate::fixtures::UNDECLARED_TASK;
use crate::html::{Trace, render, render_run};
use crate::judge::{self, Options};

#[test]
fn a_run_of_several_revision_traces_nests_its_headings_one_level_deeper() {
    let options = Options::default()
        .revisions(["2025-11-25".parse().unwrap(), "2026-07-28".parse().unwrap()]);
    let judgment = judge::judge_with(SESSION, &options).unwrap();
    let trace = |name| Trace {
        name: Some(name),
        events: &judgment.events,
    };
    let page = render_run(
        &[
            (trace("a.jsonl"), &judgment.reports[..]),
            (trace("b.jsonl"), &judgment.reports[..]),
        ],
        &[],
    );
    // Each trace's sections are h3, its revision parts h4, their clauses h5.
    assert!(page.contains("<h3 id=\"t0-findings-h\">Clauses to act on</h3>"));
    assert!(page.contains("<h4 class=\"revision-heading\">At revision 2025-11-25</h4>"));
    assert!(
        page.contains(
            "<h5 class=\"clause-id\"><a href=\"#t0-2025-11-25-BASE-003\">BASE-003</a></h5>"
        )
    );
    assert!(!page.contains("<h6"));
}

/// [`UNDECLARED_TASK`] with its `tools/call` declaring the extension, so no
/// clause of it fails.
fn declared_task() -> String {
    let declared = UNDECLARED_TASK.replace(
        r#"clientCapabilities":{"elicitation":{}},"#,
        r#"clientCapabilities":{"elicitation":{},"extensions":{"io.modelcontextprotocol/tasks":{}}},"#,
    );
    assert_ne!(
        declared, UNDECLARED_TASK,
        "the fixture's tools/call changed"
    );
    declared
}

#[test]
fn an_extension_clause_is_a_card_under_its_extension_heading() {
    let page = page(UNDECLARED_TASK);
    assert!(page.contains(
        "<h3 class=\"extension-id\"><code>io.modelcontextprotocol/tasks</code> \
         <span class=\"badge v-fail\">Fail</span></h3>"
    ));
    assert!(page.contains(
        "<article class=\"clause o-fail\" id=\"TASK-001\">\n<header class=\"clause-head\">\
         <h4 class=\"clause-id\"><a href=\"#TASK-001\">TASK-001</a></h4>"
    ));
    assert!(!page.contains("No clause of the extension failed"));
}

#[test]
fn an_extension_with_nothing_to_act_on_says_so() {
    let page = page(&declared_task());
    assert!(page.contains(
        "<code>io.modelcontextprotocol/tasks</code> <span class=\"badge v-pass\">Pass</span>"
    ));
    assert!(page.contains(
        "<p class=\"empty\">No clause of the extension failed, warned or went unjudged.</p>"
    ));
}

#[test]
fn a_wire_schema_with_no_message_is_not_observed() {
    let registry = mcp_conformance_core::requirement::Registry::builtin_2025_11_25().unwrap();
    let report = crate::engine::validate(&registry, &[]);
    let page = render(
        &[report],
        Trace {
            name: None,
            events: &[],
        },
    );
    assert!(page.contains("<p class=\"schema-line s-not-observed\">Wire schema: "));
    let page = page_of(SESSION);
    assert!(page.contains("<p class=\"schema-line s-pass\">Wire schema: "));
}

fn page_of(document: &str) -> String {
    let (reports, events) = judged(document);
    render(
        &reports,
        Trace {
            name: None,
            events: &events,
        },
    )
}

#[test]
fn the_only_flagged_switch_needs_unflagged_events_to_hide() {
    // Some events flagged: the switch is offered.
    assert!(page(SESSION).contains("class=\"only-flagged\""));
    // Every event flagged: nothing to hide.
    let both = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"1.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"1.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;
    let page = page(both);
    assert!(page.contains("2 events in capture order; 2 carry findings"));
    assert!(!page.contains("class=\"only-flagged\""));
}

#[test]
fn a_long_payload_is_cut_and_says_how_much_more_there_is() {
    let long = "x".repeat(20_000);
    let document = format!(
        "{SESSION}{{\"seq\":5,\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{{\"data\":\"{long}\"}}}}}}\n",
    );
    let page = page(&document);
    assert_eq!(
        page.matches("more bytes; the trace line holds the whole message.")
            .count(),
        1
    );
}

#[test]
fn each_clause_says_whom_it_binds_and_each_finding_whom_it_is_held_against() {
    let page = page(SESSION);
    assert!(
        page.contains(
            "<span class=\"pill level\">MUST NOT</span><span class=\"pill actor\">binds both</span>"
        ),
        "{page}"
    );
    // In the clause card and at the event on the timeline.
    assert!(
        page.contains("<span class=\"detail\"><span class=\"pill party\">client</span>request"),
        "{page}"
    );
    assert!(
        page.contains(
            "<a href=\"#BASE-003-1\">BASE-003</a><span class=\"pill party\">client</span>"
        ),
        "{page}"
    );
    assert!(!page.contains("unattributed"), "{page}");
}

#[test]
fn a_party_view_changes_the_badge_and_says_why() {
    let (mut reports, events) = judged(SESSION);
    reports[0].party_view = Some(crate::report::Party::Server);
    let page = render(
        &reports,
        Trace {
            name: Some("session.jsonl"),
            events: &events,
        },
    );
    assert!(
        page.contains("<span class=\"badge v-pass\">Pass</span>"),
        "{page}"
    );
    assert!(
        page.contains("1 clause failed; server findings only (1 client finding set aside)"),
        "{page}"
    );
    assert!(
        page.contains("<title>Pass — session.jsonl — MCP trace validation</title>"),
        "{page}"
    );
    // A finding held against nobody is labelled under the view.
    reports[0].requirements[0]
        .findings
        .push(crate::report::Finding {
            check: "x".to_owned(),
            seq: None,
            party: None,
            detail: "nobody's".to_owned(),
        });
    reports[0].requirements[0].outcome = crate::report::Outcome::Fail;
    let page = render(
        &reports,
        Trace {
            name: Some("session.jsonl"),
            events: &events,
        },
    );
    assert!(
        page.contains("<span class=\"pill party none\">unattributed</span>nobody&#39;s"),
        "{page}"
    );
}
