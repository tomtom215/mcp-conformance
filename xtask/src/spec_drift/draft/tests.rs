// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for `spec-drift --draft`: every comparison it reports, offline, on
//! fixture pages; and the committed post-release record against the registry.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use super::census::Family;
use super::compare::{CHANGED_FLOOR, CensusMove, PageDelta, census_moves, classify, page_delta};
use super::*;

/// A released page with two parallel clauses and one standalone one.
const RELEASED: &str = "\
## Headers

Constraints on `x-mcp-header` values:

- **MUST NOT** be empty
- **MUST** only be applied to parameters with primitive types (integer,
  string, boolean). Parameters with type `number` are not permitted.
  Integer values **MUST** be within the safe range for JavaScript
  (−2<sup>53</sup>+1 to 2<sup>53</sup>−1)

Servers **MUST** reject a request whose `Mcp-Method` header is missing.
Servers **MUST** reject a request whose `Mcp-Name` header is missing.

Clients **SHOULD** retry a failed request once after a short delay.
";

const INTEGER: &str = "Constraints on `x-mcp-header` values: Integer values MUST be within the safe range for JavaScript (−2<sup>53</sup>+1 to 2<sup>53</sup>−1)";
const METHOD: &str = "Servers MUST reject a request whose `Mcp-Method` header is missing.";
const NAME: &str = "Servers MUST reject a request whose `Mcp-Name` header is missing.";
const RETRY: &str = "Clients SHOULD retry a failed request once after a short delay.";

fn requirement(id: &str, section: &str, quote: &str) -> Requirement {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "level": "MUST",
        "actor": "server",
        "source": {"section": section, "quote": quote},
        "checks": ["fixture.check"]
    }))
    .unwrap()
}

fn page_set(pages: &[&str]) -> BTreeSet<String> {
    pages.iter().map(|page| (*page).to_owned()).collect()
}

#[test]
fn an_unchanged_quote_is_present() {
    let page = normalize(RELEASED);
    for quote in [INTEGER, METHOD, NAME, RETRY] {
        assert_eq!(classify(&page, quote, [METHOD, NAME]), Status::Present);
    }
}

#[test]
fn a_reworded_quote_is_changed_and_shows_the_draft_wording() {
    // TRAN-081's real post-release edit, in miniature.
    let draft = RELEASED.replace(
        "for JavaScript\n",
        "for integers\n  represented using IEEE754 double-precision floating point numbers\n",
    );
    let Status::Changed { closest, shared } = classify(&normalize(&draft), INTEGER, [RETRY]) else {
        panic!("expected a change");
    };
    assert!((CHANGED_FLOOR..100).contains(&shared), "{shared}");
    assert!(closest.contains("IEEE754 double-precision"), "{closest}");
    assert!(closest.starts_with("Integer values MUST"), "{closest}");
}

#[test]
fn a_deleted_quote_is_removed() {
    let draft = RELEASED.replace(
        "Clients **SHOULD** retry a failed request once after a short delay.\n",
        "",
    );
    assert!(matches!(
        classify(&normalize(&draft), RETRY, [METHOD, NAME]),
        Status::Removed { .. }
    ));
}

#[test]
fn a_deleted_clause_is_not_mistaken_for_its_surviving_sibling() {
    // `Mcp-Name`'s clause gone, `Mcp-Method`'s parallel one still there: the
    // sibling shares most of the deleted clause's words, and is masked
    // because another entry still quotes it verbatim.
    let draft = RELEASED.replace(
        "Servers **MUST** reject a request whose `Mcp-Name` header is missing.\n",
        "",
    );
    let page = normalize(&draft);
    assert!(matches!(
        classify(&page, NAME, [METHOD, INTEGER, RETRY]),
        Status::Removed { .. }
    ));
    // Unmasked, the same page reads as a change: the masking is what decides.
    assert!(matches!(
        classify(&page, NAME, [] as [&str; 0]),
        Status::Changed { .. }
    ));
}

#[test]
fn a_page_new_in_the_draft_and_a_page_it_dropped_are_both_named() {
    let listed: Vec<String> = ["basic/index", "server/tools", "server/utilities/ping"]
        .iter()
        .map(|page| (*page).to_owned())
        .collect();
    // `basic/index` is published as `basic`: no delta for the spelling.
    let live = page_set(&["basic", "server/tools", "server/utilities/tracing"]);
    assert_eq!(
        page_delta(&listed, &live),
        PageDelta {
            added: vec!["server/utilities/tracing".to_owned()],
            removed: vec!["server/utilities/ping".to_owned()],
        }
    );
    assert_eq!(
        page_delta(&listed[..2], &page_set(&["basic", "server/tools"])),
        PageDelta::default()
    );
}

#[test]
fn every_family_s_census_move_is_reported_with_both_numbers() {
    let committed = |pairs: &[(&str, u32)]| -> BTreeMap<String, u32> {
        pairs
            .iter()
            .map(|(page, n)| ((*page).to_owned(), *n))
            .collect()
    };
    let must = committed(&[("server/tools", 18), ("basic/index", 45)]);
    let should = committed(&[("server/tools", 18), ("basic/index", 20)]);
    let may = committed(&[("server/tools", 4)]);
    let live: BTreeMap<String, Tally> = [
        (
            "basic/index",
            Tally {
                must: 45,
                should: 20,
                may: 11,
            },
        ),
        (
            "server/tools",
            Tally {
                must: 19,
                should: 18,
                may: 4,
            },
        ),
    ]
    .into_iter()
    .map(|(page, tally)| (page.to_owned(), tally))
    .collect();
    let moves = census_moves(
        [
            (Family::Must, &must),
            (Family::Should, &should),
            (Family::May, &may),
        ],
        &live,
    );
    assert_eq!(
        moves,
        [
            CensusMove {
                page: "basic/index".to_owned(),
                family: Family::May,
                committed: None,
                live: 11,
            },
            CensusMove {
                page: "server/tools".to_owned(),
                family: Family::Must,
                committed: Some(18),
                live: 19,
            },
        ]
    );
}

fn record(changes: &serde_json::Value) -> PostRelease {
    serde_json::from_value(serde_json::json!({
        "_policy": "fixture",
        "base": BASE,
        "changes": changes
    }))
    .unwrap()
}

fn fixture_entries() -> [Requirement; 4] {
    [
        requirement("TRAN-081", "x/headers#a", INTEGER),
        requirement("TRAN-001", "x/headers#a", METHOD),
        requirement("TRAN-002", "x/headers#a", RETRY),
        requirement("PING-001", "x/ping#a", "Receivers MUST respond promptly."),
    ]
}

fn fixture_pages(entries: &[Requirement; 4]) -> BTreeMap<String, Vec<&Requirement>> {
    [
        ("x/headers".to_owned(), entries[..3].iter().collect()),
        ("x/ping".to_owned(), entries[3..].iter().collect()),
    ]
    .into_iter()
    .collect()
}

/// The draft of `RELEASED`: TRAN-081 reworded, TRAN-002's clause deleted, and
/// `x/ping` absent because the draft no longer publishes it.
fn fixture_draft() -> BTreeMap<String, String> {
    let draft = RELEASED
        .replace("for JavaScript\n", "for IEEE754 doubles\n")
        .replace(
            "Clients **SHOULD** retry a failed request once after a short delay.\n",
            "",
        );
    std::iter::once(("x/headers".to_owned(), draft)).collect()
}

#[test]
fn findings_cover_every_status_and_confirm_a_recorded_change() {
    let entries = fixture_entries();
    let recorded = record(&serde_json::json!([{
        "id": "TRAN-081",
        "released": INTEGER,
        "draft": INTEGER.replace("for JavaScript", "for IEEE754 doubles"),
        "recorded": "2026-10-13",
        "reading": "fixture"
    }]));
    let (found, stale) = findings(&fixture_pages(&entries), &fixture_draft(), &recorded);
    let status = |id: &str| {
        let finding = found
            .iter()
            .find(|f| f.requirement.id.as_str() == id)
            .unwrap();
        (finding.status.clone(), finding.recorded)
    };
    assert!(matches!(status("TRAN-081"), (Status::Changed { .. }, true)));
    assert_eq!(status("TRAN-001"), (Status::Present, false));
    assert!(matches!(
        status("TRAN-002"),
        (Status::Removed { .. }, false)
    ));
    assert_eq!(status("PING-001"), (Status::PageRemoved, false));
    assert!(stale.is_empty(), "{stale:?}");
}

#[test]
fn a_record_the_draft_no_longer_bears_out_is_stale_and_reported() {
    // A row whose draft wording the page does not carry is stale, and so is
    // one whose released wording is back.
    let entries = fixture_entries();
    let wrong = record(&serde_json::json!([
        {"id": "TRAN-081", "released": INTEGER, "draft": "Something else MUST hold.",
         "recorded": "2026-10-13", "reading": "fixture"},
        {"id": "TRAN-001", "released": METHOD, "draft": METHOD,
         "recorded": "2026-10-13", "reading": "fixture"}
    ]));
    let (found, stale) = findings(&fixture_pages(&entries), &fixture_draft(), &wrong);
    assert_eq!(stale, ["TRAN-081", "TRAN-001"], "in page order");
    let report = Report {
        findings: found,
        stale_records: stale,
        ..Report::default()
    };
    assert_eq!(
        render::totals(&report),
        render::Totals {
            present: 1,
            changed: 1,
            recorded: 0,
            removed: 1,
            page_removed: 1,
        }
    );
    let markdown = render::markdown(&report);
    for expected in [
        "4 checked: 1 present, 1 changed (0 recorded), 1 removed, 1 on a page the draft dropped",
        "**TRAN-002** removed from `x/headers`",
        "**PING-001**: the draft no longer publishes `x/ping`",
        "TRAN-001: recorded in `post-release.json`, but the draft no longer bears",
    ] {
        assert!(markdown.contains(expected), "{expected}\n{markdown}");
    }
}

#[test]
fn the_committed_record_names_real_entries_by_their_released_quotes() {
    let set = RegistrySet::builtin().unwrap();
    let registry = set.registry(BASE.parse().unwrap()).unwrap();
    let committed = load_post_release().unwrap();
    assert_eq!(record_problems(&committed, &registry), Vec::<String>::new());
    assert!(
        committed
            .changes
            .iter()
            .any(|change| change.id == "TRAN-081")
    );

    let mut drifted = load_post_release().unwrap();
    drifted.base = "2025-11-25".to_owned();
    drifted.changes[0].released.push_str(" (edited)");
    drifted.changes.push(Change {
        id: "TRAN-999".to_owned(),
        released: String::new(),
        draft: String::new(),
        recorded: String::new(),
        reading: String::new(),
    });
    let problems = record_problems(&drifted, &registry);
    assert_eq!(problems.len(), 3, "{problems:?}");
}

#[test]
fn the_skeleton_is_well_formed_and_no_dated_revision_reads_it() {
    let skeleton = load_sources(DRAFT).unwrap();
    assert_eq!(pages::structure(DRAFT, &skeleton), Vec::<String>::new());
    // Data only: `draft` names no revision a registry set could describe, and
    // the release gate's revision list does not carry it.
    assert!(
        DRAFT
            .parse::<mcp_conformance_core::revision::ProtocolRevision>()
            .is_err()
    );
    assert!(!super::super::REVISIONS.contains(&DRAFT));
    assert!(super::super::REVISIONS.contains(&BASE));
}
