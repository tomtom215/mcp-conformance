// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the census: keyword counting per family, and coverage by quotes.

#![allow(clippy::unwrap_used)]

use super::*;

fn tally(must: u32, should: u32, may: u32) -> Tally {
    Tally { must, should, may }
}

#[test]
fn counts_whole_uppercase_keywords_only() {
    assert_eq!(count("The client **MUST** send it."), tally(1, 0, 0));
    // MUST NOT is one clause, not a MUST plus a word.
    assert_eq!(count("The client **MUST NOT** send it."), tally(1, 0, 0));
    assert_eq!(
        count("A **MUST** here and a **MUST NOT** there."),
        tally(2, 0, 0)
    );
    assert_eq!(count("**SHALL** and **SHALL NOT**"), tally(2, 0, 0));
    // RFC 8174: only the uppercase instance is normative.
    assert_eq!(
        count("the client must send it, and should too, if it may"),
        tally(0, 0, 0)
    );
    // Not a substring of a longer word or identifier.
    assert_eq!(
        count("MUSTARD and SHALLOW and RE_MUST_X and MISSING_REQUIRED_CLIENT_CAPABILITY"),
        tally(0, 0, 0)
    );
}

#[test]
fn every_family_member_counts_in_its_own_family() {
    assert_eq!(count("These headers are **REQUIRED**."), tally(1, 0, 0));
    assert_eq!(
        count("It **SHOULD**, it **SHOULD NOT**, it is **RECOMMENDED**."),
        tally(0, 3, 0)
    );
    // NOT RECOMMENDED is one SHOULD-family keyword, not a RECOMMENDED.
    assert_eq!(count("That is **NOT RECOMMENDED**."), tally(0, 1, 0));
    assert_eq!(
        count("It **MAY**; the field is **OPTIONAL**."),
        tally(0, 0, 2)
    );
}

#[test]
fn frontmatter_and_code_are_not_prose_but_tables_are() {
    let page = "---\ntitle: MUST not count\n---\n\nThe client **MUST** send it.\n\n\
                ```json\n{\"note\": \"MUST NOT count\"}\n```\n\nAnd a **MUST NOT** after.\n\n\
                | Case | Client |\n| --- | --- |\n| Null | Client MUST omit it |\n";
    assert_eq!(count(page), tally(3, 0, 0));
}

#[test]
fn a_line_wrapped_keyword_still_counts_once() {
    // The published pages wrap at 80 columns, so a keyword and the words
    // around it are routinely split across lines; normalization joins them.
    assert_eq!(
        count("the client\n**MUST**\ninclude the header"),
        tally(1, 0, 0)
    );
    assert_eq!(
        count("the client **MUST**\n**NOT** do that"),
        tally(1, 0, 0)
    );
}

#[test]
fn a_keyword_is_covered_only_inside_a_quote_from_the_page() {
    let page = prose(
        "Servers **MUST** answer. Clients **SHOULD** retry.\n\n\
         These headers are **REQUIRED** for compliance. Clients **MAY** wait.\n",
    );
    let missing = uncovered(&page, ["Servers MUST answer."]);
    assert_eq!(missing.len(), 2, "{missing:#?}");
    assert!(missing[0].starts_with("SHOULD-family \"SHOULD\" has no entry"));
    assert!(missing[1].starts_with("MUST-family \"REQUIRED\" has no entry"));
    // MAY-family is counted, never required to be entered.
    assert!(missing.iter().all(|line| !line.contains("\"MAY\"")));
    let all = [
        "Servers MUST answer.",
        "Clients SHOULD retry.",
        "These headers are REQUIRED for compliance.",
    ];
    assert_eq!(uncovered(&page, all), Vec::<String>::new());
    // A quote the page does not carry covers nothing.
    assert_eq!(
        uncovered(&page, ["Servers MUST reply."]).len(),
        3,
        "a drifted quote must not cover the clause it used to"
    );
}

#[test]
fn list_and_table_quotes_cover_their_keywords() {
    let page = prose(
        "Clients **SHOULD**:\n\n- Prompt the user\n- Log usage\n\n\
         | Value | Client | Server |\n| --- | --- | --- |\n\
         | `null` | Client MUST omit the header | Server MUST NOT expect the header |\n",
    );
    let quotes = [
        "Clients SHOULD: Prompt the user; Log usage",
        "`null` | Client MUST omit the header",
    ];
    let missing = uncovered(&page, quotes);
    // The server cell is a second clause the quotes above do not reach.
    assert_eq!(missing.len(), 1, "{missing:#?}");
    assert!(missing[0].contains("\"MUST NOT\""), "{missing:#?}");
}

#[test]
fn reconcile_names_both_numbers_and_the_missing_pages() {
    let committed = BTreeMap::from([("a".to_owned(), 3), ("gone".to_owned(), 1)]);
    let live = BTreeMap::from([
        ("a".to_owned(), tally(4, 0, 0)),
        ("new".to_owned(), tally(2, 0, 0)),
    ]);
    let problems = reconcile("r", Family::Must, &committed, &live).unwrap_err();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|p| p.contains("now carries 4") && p.contains("says 3"))
    );
    assert!(
        problems
            .iter()
            .any(|p| p.contains("r/new") && p.contains("no `must_census` entry"))
    );
    assert!(
        problems
            .iter()
            .any(|p| p.contains("r/gone") && p.contains("not an in-scope page"))
    );

    let agreed = BTreeMap::from([("a".to_owned(), 3)]);
    let live = BTreeMap::from([("a".to_owned(), tally(3, 5, 7))]);
    assert_eq!(reconcile("r", Family::Must, &agreed, &live).unwrap(), 3);
    // Each family is held to its own committed map.
    let problems = reconcile("r", Family::Should, &agreed, &live).unwrap_err();
    assert!(
        problems[0].contains("5 SHOULD-family") && problems[0].contains("`should_census` says 3"),
        "{problems:?}"
    );
}
