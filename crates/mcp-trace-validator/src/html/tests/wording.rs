// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the page says at the edges of its wording: the headline at exactly no,
//! one and two rules, a verdict whose findings are all set aside, a session
//! only the schema refuses, the run's count of recordings to look at, a rule
//! this build cannot check in the bars and the table by area, and the reason a
//! baseline accepts a finding.

use mcp_conformance_core::requirement::Registry;

use super::SESSION;
use crate::fixtures::baselined_sdk_session;
use crate::html::{Trace, render, render_run};
use crate::reader::{Limits, parse_trace};
use crate::report::Report;

/// A registry of `rows`: an ID and the check that judges it. A `PROM` row is
/// gated on a capability the sessions here never declare.
fn registry(rows: &[(&str, &str)]) -> Registry {
    let rows: Vec<String> = rows
        .iter()
        .map(|(id, check)| {
            let gate = if id.starts_with("PROM") {
                r#""capability": "server.prompts","#
            } else {
                ""
            };
            format!(
                r#"{{"id": "{id}", "level": "MUST", "actor": "both", {gate}
                    "source": {{"section": "basic/index#messages", "quote": "MUST x"}},
                    "checks": ["{check}"]}}"#
            )
        })
        .collect();
    Registry::from_json(&format!(
        r#"{{"revision": "2025-11-25", "requirements": [{}]}}"#,
        rows.join(",")
    ))
    .unwrap()
}

/// `count` rules every JSON-RPC 2.0 message keeps.
fn rules(count: usize) -> Registry {
    let ids: Vec<String> = (1..=count).map(|n| format!("BASE-00{n}")).collect();
    let rows: Vec<(&str, &str)> = ids
        .iter()
        .map(|id| (id.as_str(), "base.jsonrpc-version"))
        .collect();
    registry(&rows)
}

/// The first `lines` events of [`SESSION`]: none, or its three-message handshake.
fn session(lines: usize) -> String {
    SESSION.lines().take(lines).collect::<Vec<_>>().join("\n")
}

/// `document` judged by `registry`, and the page of it.
fn judged(registry: &Registry, document: &str) -> (Report, String) {
    let events = parse_trace(document, &Limits::default()).unwrap();
    let report = crate::engine::validate(registry, &events);
    let page = render(
        core::slice::from_ref(&report),
        Trace {
            name: None,
            events: &events,
        },
    );
    (report, page)
}

fn page(registry: &Registry, document: &str) -> String {
    judged(registry, document).1
}

const NOTHING_TO_FIX: &str = "<strong>What to do next</strong> Nothing to fix.</p>";

#[test]
fn a_pass_says_how_many_rules_held_at_none_one_and_two() {
    let none = page(&rules(1), "");
    assert!(
        none.contains(
            "<p class=\"headline\">No rule was broken, but none could be checked against this \
             session.</p>"
        ),
        "{none}"
    );
    let one = page(&rules(1), &session(3));
    assert!(
        one.contains(
            "<p class=\"headline\">No rule was broken: the one rule this session could be \
             checked against held.</p>"
        ),
        "{one}"
    );
    let two = page(&rules(2), &session(3));
    assert!(
        two.contains(
            "<p class=\"headline\">No rule was broken: all 2 rules this session could be \
             checked against held.</p>"
        ),
        "{two}"
    );
}

#[test]
fn a_pass_names_the_rules_that_never_came_up_only_when_there_are_some() {
    // Every rule was checked: there is nothing more to say.
    let all_seen = page(&rules(2), &session(3));
    assert!(all_seen.contains(NOTHING_TO_FIX), "{all_seen}");
    assert!(!all_seen.contains("never came up"), "{all_seen}");
    // One and two rules the session never reached.
    let one = page(&rules(1), "");
    assert!(
        one.contains(
            "Nothing to fix. A pass covers only what this session did: 1 rule never came up in it."
        ),
        "{one}"
    );
    let two = page(&rules(2), "");
    assert!(
        two.contains(
            "Nothing to fix. A pass covers only what this session did: 2 rules never came up in \
             it."
        ),
        "{two}"
    );
    assert!(two.contains("(<a href=\"#accounting\">Accounting</a> lists them)"));
}

#[test]
fn a_pass_whose_findings_are_all_set_aside_does_not_claim_every_rule_held() {
    let known = baselined_sdk_session("known");
    let page = render(
        &known.reports,
        Trace {
            name: None,
            events: &known.events,
        },
    );
    assert!(page.contains("<span class=\"badge v-pass\">Pass</span>"));
    assert!(
        page.contains(
            "<p class=\"headline\">No finding counts against this session here: the ones it has \
             are set aside, as the line below says.</p>"
        ),
        "{page}"
    );
    assert!(!page.contains("No rule was broken"), "{page}");
}

/// The handshake of [`SESSION`] with an `initialize` request the protocol's
/// schema refuses (no `clientInfo`), speaking `version` of JSON-RPC.
fn refused_by_the_schema(version: &str) -> String {
    let document = session(3)
        .replace(r#","clientInfo":{"name":"c","version":"0"}"#, "")
        .replacen(
            r#""jsonrpc":"2.0""#,
            &format!(r#""jsonrpc":"{version}""#),
            1,
        );
    assert_ne!(document, session(3), "the fixture's initialize changed");
    document
}

#[test]
fn a_session_only_the_schema_refuses_is_not_said_to_have_broken_a_rule() {
    let (report, page) = judged(&rules(1), &refused_by_the_schema("2.0"));
    assert_eq!(report.totals.fail, 0, "{page}");
    assert!(page.contains("<span class=\"badge v-fail\">Fail</span>"));
    assert!(
        page.contains(
            "<p class=\"headline\">This session sent 1 message that does not match the \
             protocol&#39;s schema.</p>"
        ),
        "{page}"
    );
    // With a rule broken as well, both are named.
    let (report, page) = judged(&rules(1), &refused_by_the_schema("1.0"));
    assert_eq!(report.totals.fail, 1, "{page}");
    assert!(
        page.contains(
            "<p class=\"headline\">This session broke 1 MUST-level rule and sent 1 message that \
             does not match the protocol&#39;s schema.</p>"
        ),
        "{page}"
    );
}

#[test]
fn a_run_counts_the_recordings_that_are_not_a_pass() {
    let registry = rules(1);
    let pass_events = parse_trace(&session(3), &Limits::default()).unwrap();
    let fail_events = parse_trace(&refused_by_the_schema("1.0"), &Limits::default()).unwrap();
    let passing = [crate::engine::validate(&registry, &pass_events)];
    let failing = [crate::engine::validate(&registry, &fail_events)];
    let pass = |name| {
        (
            Trace {
                name: Some(name),
                events: &pass_events,
            },
            &passing[..],
        )
    };
    let fail = |name| {
        (
            Trace {
                name: Some(name),
                events: &fail_events,
            },
            &failing[..],
        )
    };
    let headline = |traces: &[(Trace<'_>, &[Report])]| {
        let page = render_run(traces, &[]);
        let start = page.find("<p class=\"headline\">").unwrap();
        let end = start + page[start..].find("</p>").unwrap();
        page[start + "<p class=\"headline\">".len()..end].to_owned()
    };
    assert_eq!(
        headline(&[pass("a"), pass("b"), pass("c")]),
        "No rule was broken in any of the 3 recordings."
    );
    assert_eq!(
        headline(&[pass("a"), pass("b"), fail("c")]),
        "1 of 3 recordings has something to look at."
    );
    assert_eq!(
        headline(&[pass("a"), fail("b"), fail("c")]),
        "2 of 3 recordings have something to look at."
    );
    assert_eq!(
        headline(&[fail("a"), fail("b"), fail("c")]),
        "3 of 3 recordings have something to look at."
    );
}

/// A registry with a rule this build has no check for, beside rules it has.
fn with_an_unknown_check() -> Registry {
    registry(&[
        ("BASE-001", "base.jsonrpc-version"),
        ("FUTR-001", "future.not-built-yet"),
        ("FUTR-002", "base.jsonrpc-version"),
        ("TASK-001", "base.jsonrpc-version"),
        ("PROM-001", "base.jsonrpc-version"),
    ])
}

const UNSUPPORTED_COLUMN: &str = "<th scope=\"col\" class=\"num\">Unsupported</th>";

#[test]
fn a_rule_this_build_cannot_check_is_counted_drawn_and_said_in_plain_words() {
    let (report, page) = judged(&with_an_unknown_check(), &session(3));
    assert_eq!(
        (report.totals.unsupported, report.totals.not_applicable),
        (1, 1),
        "{page}"
    );
    // The headline, the card and the bar of what was not checked: the
    // unsupported rule and the gated one.
    assert!(
        page.contains(
            "<p class=\"headline\">1 rule could not be checked by this version of the tool, so \
             the result is not a pass.</p>"
        ),
        "{page}"
    );
    assert!(
        page.contains("<p class=\"clause-lead\">This rule could not be checked.</p>"),
        "{page}"
    );
    assert!(
        page.contains("<strong>3 rules</strong> checked against this session"),
        "{page}"
    );
    assert!(
        page.contains("<strong>2 rules</strong> not checked here; the counts below say why"),
        "{page}"
    );
    // The table by area has a column for it, and the area's row its count and
    // half its bar; the row is not dimmed, for it has something to act on.
    assert!(page.contains(UNSUPPORTED_COLUMN), "{page}");
    assert!(
        page.contains(
            "<tr><th scope=\"row\"><code class=\"area-id\">FUTR</code></th>\
             <td class=\"num n-fail zero\">0</td><td class=\"num n-warn zero\">0</td>\
             <td class=\"num n-pass\">1</td><td class=\"num n-unsupported\">1</td>\
             <td class=\"num n-unchecked zero\">0</td><td class=\"bar-col\">\
             <svg class=\"segbar\" viewBox=\"0 0 1000 10\" preserveAspectRatio=\"none\" \
             aria-hidden=\"true\" focusable=\"false\">\
             <rect class=\"s-unsupported\" x=\"0\" y=\"0\" width=\"500\" height=\"10\"/>\
             <rect class=\"s-pass\" x=\"500\" y=\"0\" width=\"500\" height=\"10\"/></svg></td></tr>"
        ),
        "{page}"
    );
}

#[test]
fn the_table_by_area_names_its_areas_and_dims_only_the_unchecked_ones() {
    let page = page(&with_an_unknown_check(), &session(3));
    // A core rule of an external registry in the tasks area has its name.
    assert!(
        page.contains(
            "<tr><th scope=\"row\"><span class=\"area-name\">Tasks</span> \
             <code class=\"area-id\">TASK</code></th><td class=\"num n-fail zero\">0</td>\
             <td class=\"num n-warn zero\">0</td><td class=\"num n-pass\">1</td>\
             <td class=\"num n-unsupported zero\">0</td>\
             <td class=\"num n-unchecked zero\">0</td>"
        ),
        "{page}"
    );
    // An area with nothing checked in it is dimmed.
    assert!(
        page.contains(
            "<tr class=\"quiet\"><th scope=\"row\"><span class=\"area-name\">Prompts</span> \
             <code class=\"area-id\">PROM</code></th><td class=\"num n-fail zero\">0</td>\
             <td class=\"num n-warn zero\">0</td><td class=\"num n-pass zero\">0</td>\
             <td class=\"num n-unsupported zero\">0</td><td class=\"num n-unchecked\">1</td>"
        ),
        "{page}"
    );
    assert_eq!(page.matches("<tr class=\"quiet\">").count(), 1, "{page}");
}

#[test]
fn the_table_by_area_has_no_unsupported_column_when_every_rule_has_its_check() {
    let page = page(&rules(2), &session(3));
    assert!(!page.contains(UNSUPPORTED_COLUMN), "{page}");
    assert!(!page.contains("<td class=\"num n-unsupported"), "{page}");
    assert!(
        page.contains("<td class=\"num n-pass\">2</td><td class=\"num n-unchecked zero\">0</td>"),
        "{page}"
    );
}

#[test]
fn an_accepted_finding_has_the_baselines_reason_as_a_line_of_its_own() {
    let mut known = baselined_sdk_session("the generator emits <draft-07>");
    let page_of = |reports: &[Report]| {
        render(
            reports,
            Trace {
                name: None,
                events: &known.events,
            },
        )
    };
    let page = page_of(&known.reports);
    assert_eq!(
        page.matches(
            "<span class=\"check\">accepted by baseline: the generator emits \
             &lt;draft-07&gt;</span>"
        )
        .count(),
        2,
        "one line per accepted finding: {page}"
    );
    // An acceptance that lapses says through when.
    for row in &mut known.reports[0].requirements {
        for finding in &mut row.findings {
            if let Some(acceptance) = &mut finding.baselined {
                acceptance.expires = Some("2030-01-31".to_owned());
            }
        }
    }
    let page = page_of(&known.reports);
    assert_eq!(
        page.matches(
            "<span class=\"check\">accepted by baseline: the generator emits \
             &lt;draft-07&gt; (through 2030-01-31)</span>"
        )
        .count(),
        2,
        "{page}"
    );
}
