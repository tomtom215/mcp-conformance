// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Who broke it, in `JUnit`: the `actor` and `party` properties, each finding's
//! party in its location, and a party view's set-aside cases and counts.

use super::report_for;
use crate::junit::text::properties;
use crate::junit::{Options, render, render_with};
use crate::report::Party;

/// A session whose client reuses id 1 (BASE-003, the client's) and whose
/// server answers `initialize` without `serverInfo` (LIFE-010, the server's).
const TWO_PARTIES: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#;

/// The `failures` attribute of the first `<testsuite>`.
fn suite_failures(xml: &str) -> u32 {
    xml.lines()
        .find(|line| line.starts_with("  <testsuite "))
        .and_then(|line| {
            let at = line.find("failures=\"")? + 10;
            line[at..].split('"').next()?.parse::<u32>().ok()
        })
        .unwrap()
}

#[test]
fn a_failing_case_names_the_actor_and_the_party_and_each_finding_its_party() {
    let xml = render(&report_for(TWO_PARTIES));
    assert!(
        xml.contains(
            "<testcase classname=\"mcp.2025-11-25\" name=\"BASE-003 (MUST NOT)\">\n      \
             <properties><property name=\"actor\" value=\"both\"/><property name=\"party\" \
             value=\"client\"/></properties>\n      <failure message=\"request &quot;ping&quot; \
             reuses id 1, already used by the same party at seq 0\">[base.request-id-unique] at \
             seq 3 (client)"
        ),
        "{xml}"
    );
    assert!(
        xml.contains(
            "<properties><property name=\"actor\" value=\"server\"/><property name=\"party\" \
             value=\"server\"/></properties>"
        ),
        "{xml}"
    );
    // The wire schema binds no actor; its findings still name their party.
    assert!(
        xml.contains(
            "name=\"wire schema\">\n      <properties><property name=\"party\" \
             value=\"server\"/></properties>"
        ),
        "{xml}"
    );
}

#[test]
fn a_party_view_sets_the_other_partys_failures_aside_and_counts_accordingly() {
    let mut report = report_for(TWO_PARTIES);
    // BASE-003 (client), LIFE-010 (server), the wire schema (server).
    assert_eq!(suite_failures(&render(&report)), 3);

    report.party_view = Some(Party::Server);
    let server = render(&report);
    assert_eq!(suite_failures(&server), 2, "{server}");
    assert!(
        server.contains(
            "name=\"BASE-003 (MUST NOT)\">\n      <properties><property name=\"actor\" \
             value=\"both\"/><property name=\"party\" value=\"client\"/></properties>\n      \
             <system-out>set aside: this report counts server findings only, and every finding \
             here is held against the client\n[base.request-id-unique] at seq 3 (client): \
             request &quot;ping&quot; reuses id 1"
        ),
        "{server}"
    );

    report.party_view = Some(Party::Client);
    let client = render(&report);
    assert_eq!(suite_failures(&client), 1, "{client}");
    assert!(
        client.contains(
            "name=\"wire schema\">\n      <properties><property name=\"party\" \
             value=\"server\"/></properties>\n      <system-out>set aside: this report counts \
             client findings only, and every finding here is held against the server\n\
             [wire-schema] at seq 1 (server): "
        ),
        "{client}"
    );
    // A set-aside row is never promoted by --strict either.
    let strict = render_with(
        core::slice::from_ref(&report),
        &Options::default().strict(true),
    );
    assert_eq!(suite_failures(&strict), 1, "{strict}");
}

#[test]
fn the_properties_list_each_party_once_in_order() {
    use crate::report::Finding;
    let finding = |party| Finding {
        check: "x".to_owned(),
        seq: None,
        party,
        detail: String::new(),
    };
    let mut out = String::new();
    properties(
        &mut out,
        Some("both"),
        &[
            finding(Some(Party::Server)),
            finding(None),
            finding(Some(Party::Client)),
            finding(Some(Party::Server)),
        ],
    );
    assert_eq!(
        out,
        "      <properties><property name=\"actor\" value=\"both\"/><property name=\"party\" \
         value=\"client, server\"/></properties>\n"
    );
    let mut out = String::new();
    properties(&mut out, None, &[finding(None)]);
    assert_eq!(out, "      <properties></properties>\n");
}
