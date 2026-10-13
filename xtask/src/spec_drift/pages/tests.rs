// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the page-set check.

#![allow(clippy::unwrap_used)]

use super::*;

/// The pages the site navigation published for each final revision, read from
/// `docs/docs.json` at upstream `1f52096` (2026-10-10). Both revisions are
/// released, so their page sets are fixed; the weekly drift run compares the
/// live navigation, and this copy exists only so `cargo test` can fail offline
/// when a page drops out of both lists.
const PUBLISHED: &[(&str, &[&str])] = &[
    (
        "2025-11-25",
        &[
            "architecture/index",
            "basic/authorization",
            "basic/index",
            "basic/lifecycle",
            "basic/transports",
            "basic/utilities/cancellation",
            "basic/utilities/ping",
            "basic/utilities/progress",
            "basic/utilities/tasks",
            "changelog",
            "client/elicitation",
            "client/roots",
            "client/sampling",
            "index",
            "schema",
            "server/index",
            "server/prompts",
            "server/resources",
            "server/tools",
            "server/utilities/completion",
            "server/utilities/logging",
            "server/utilities/pagination",
        ],
    ),
    (
        "2026-07-28",
        &[
            "architecture/index",
            "basic/authorization/authorization-server-discovery",
            "basic/authorization/client-registration",
            "basic/authorization/index",
            "basic/authorization/security-considerations",
            "basic/index",
            "basic/patterns/cancellation",
            "basic/patterns/index",
            "basic/patterns/mrtr",
            "basic/patterns/progress",
            "basic/patterns/subscriptions",
            "basic/transports/index",
            "basic/transports/stdio",
            "basic/transports/streamable-http",
            "basic/versioning",
            "changelog",
            "client/elicitation",
            "client/roots",
            "client/sampling",
            "deprecated",
            "index",
            "schema",
            "server/discover",
            "server/index",
            "server/prompts",
            "server/resources",
            "server/tools",
            "server/utilities/caching",
            "server/utilities/completion",
            "server/utilities/logging",
            "server/utilities/pagination",
        ],
    ),
];

/// A navigation document listing `pages` under `revision`, nested the way the
/// site's is: a version holding pages and a group holding more.
fn navigation_for(revision: &str, pages: &[&str]) -> String {
    let (top, grouped) = pages.split_at(pages.len() / 2);
    let path = |page: &&str| Value::String(format!("specification/{revision}/{page}"));
    serde_json::json!({
        "navigation": {"tabs": [{"tab": "Specification", "versions": [{
            "version": revision,
            "pages": top.iter().map(path).chain([serde_json::json!({
                "group": "Nested",
                "pages": grouped.iter().map(path).collect::<Vec<_>>()
            })]).collect::<Vec<_>>()
        }]}]},
        "redirects": [{"source": format!("/specification/{revision}/gone"), "destination": "/"}]
    })
    .to_string()
}

fn pinned(revision: &str) -> &'static [&'static str] {
    PUBLISHED
        .iter()
        .find(|(pinned, _)| *pinned == revision)
        .map(|(_, pages)| *pages)
        .unwrap()
}

#[test]
fn every_published_page_is_in_exactly_one_list() {
    // The regression this module exists for: on 2026-10-12 three `2026-07-28`
    // pages were in neither list. Any page dropped from both fails here.
    assert_eq!(REVISIONS, ["2025-11-25", "2026-07-28"], "pin each revision");
    for revision in REVISIONS {
        let sources = load_sources(revision).unwrap();
        let navigation = navigation_for(revision, pinned(revision));
        assert_eq!(
            verify(revision, &sources, &navigation),
            Vec::<String>::new(),
            "{revision}"
        );
    }
}

#[test]
fn a_page_removed_from_both_lists_is_reported_by_name() {
    for revision in REVISIONS {
        let committed = load_sources(revision).unwrap();
        let navigation = navigation_for(revision, pinned(revision));
        let pages: Vec<String> = committed
            .in_scope
            .keys()
            .chain(committed.out_of_scope.keys())
            .cloned()
            .collect();
        for page in pages {
            let mut sources = load_sources(revision).unwrap();
            sources.in_scope.remove(&page);
            sources.out_of_scope.remove(&page);
            sources.must_census.remove(&page);
            sources.should_census.remove(&page);
            sources.may_census.remove(&page);
            let problems = verify(revision, &sources, &navigation);
            assert_eq!(problems.len(), 1, "{revision}/{page}: {problems:?}");
            assert!(
                problems[0].contains(canonical(&page)) && problems[0].contains("in neither"),
                "{problems:?}"
            );
        }
    }
}

#[test]
fn a_listed_page_the_revision_does_not_publish_is_reported() {
    let revision = "2026-07-28";
    let mut sources = load_sources(revision).unwrap();
    sources
        .out_of_scope
        .insert("basic/utilities/ping".to_owned(), "Removed.".to_owned());
    let problems = verify(
        revision,
        &sources,
        &navigation_for(revision, pinned(revision)),
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("publishes no such page"),
        "{problems:?}"
    );
}

#[test]
fn navigation_is_read_per_revision_at_any_depth_and_index_folds() {
    let navigation = navigation_for("2026-07-28", &["index", "basic/index", "server/tools"]);
    let pages = published(&navigation, "2026-07-28").unwrap();
    assert_eq!(
        pages.into_iter().collect::<Vec<_>>(),
        ["basic", "index", "server/tools"]
    );
    // Another revision's pages and the redirect table are not this revision's.
    assert!(published(&navigation, "2025-11-25").is_err());
    assert!(published("not json", "2026-07-28").is_err());
}

#[test]
fn structure_reports_overlap_missing_reasons_wrong_files_and_census_gaps() {
    let mut sources = load_sources("2025-11-25").unwrap();
    assert_eq!(structure("r", &sources), Vec::<String>::new());
    // `basic/index` is the in-scope `basic`, spelled the other way.
    sources
        .out_of_scope
        .insert("basic/index".to_owned(), "Overlap.".to_owned());
    sources
        .out_of_scope
        .insert("changelog".to_owned(), "  ".to_owned());
    sources
        .in_scope
        .insert("server/tools".to_owned(), "server/prompts.mdx".to_owned());
    sources.must_census.remove("basic/lifecycle");
    sources.must_census.insert("schema".to_owned(), 0);
    // Every family's census is held to the page set, not only the MUST one.
    sources.should_census.remove("server/tools");
    sources.may_census.insert("index".to_owned(), 0);
    let problems = structure("r", &sources);
    assert_eq!(problems.len(), 7, "{problems:#?}");
    for expected in [
        "r/basic/index: listed in `out_of_scope` and already in `in_scope`",
        "r/changelog: `out_of_scope` without a reason",
        "r/server/tools: `in_scope` names \"server/prompts.mdx\"",
        "r/basic/lifecycle: in scope with no `must_census` entry",
        "r/schema: has a `must_census` entry but is not in scope",
        "r/server/tools: in scope with no `should_census` entry",
        "r/index: has a `may_census` entry but is not in scope",
    ] {
        assert!(
            problems.iter().any(|problem| problem.starts_with(expected)),
            "{expected}: {problems:#?}"
        );
    }
}

#[test]
fn the_offline_gate_passes_on_the_committed_lists() {
    assert!(gate());
}
