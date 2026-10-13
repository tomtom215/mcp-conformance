// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the extension page set.

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::requirement::{Registry, RegistrySet};

use super::*;

fn builtin() -> Registry {
    RegistrySet::builtin()
        .unwrap()
        .registry(REVISION.parse().unwrap())
        .unwrap()
}

fn committed() -> ExtensionSources {
    load().unwrap()
}

#[test]
fn the_committed_lists_agree_with_the_builtin_entries() {
    assert_eq!(structure(&committed(), &builtin()), Vec::<String>::new());
}

#[test]
fn an_extension_without_its_page_or_entries_is_reported() {
    let mut sources = committed();
    let tasks = sources
        .extensions
        .get_mut("io.modelcontextprotocol/tasks")
        .unwrap();
    // A page listed in scope that no entry cites, and the cited page dropped.
    let cited = tasks.in_scope.pop_first().unwrap();
    tasks.in_scope.insert(
        "specification/2026-07-28/other".to_owned(),
        "other.md".to_owned(),
    );
    let problems = structure(&sources, &builtin());
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains(&format!("cites {}", cited.0))),
        "{problems:#?}"
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("no entry of io.modelcontextprotocol/tasks cites it")),
        "{problems:#?}"
    );
    // The census must follow the in-scope list.
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("in scope with no `must_census` entry")),
        "{problems:#?}"
    );

    let mut sources = committed();
    sources.extensions.remove("io.modelcontextprotocol/skills");
    let problems = structure(&sources, &builtin());
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("names extension io.modelcontextprotocol/skills")),
        "{problems:#?}"
    );
}

#[test]
fn an_unreasoned_exclusion_is_reported() {
    let mut sources = committed();
    sources
        .site_out_of_scope
        .insert("extensions/new/overview".to_owned(), " ".to_owned());
    let problems = structure(&sources, &builtin());
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].starts_with("extensions/new/overview"),
        "{problems:#?}"
    );
}

#[test]
fn the_site_navigation_is_held_to_the_lists_both_ways() {
    let sources = committed();
    let mut pages: Vec<&str> = sources
        .extensions
        .values()
        .map(|extension| extension.site_page.as_str())
        .chain(sources.site_out_of_scope.keys().map(String::as_str))
        .collect();
    let navigation =
        |pages: &[&str]| serde_json::json!({ "tabs": [{ "pages": pages }] }).to_string();
    assert_eq!(
        site_coverage(&sources, &navigation(&pages)),
        Vec::<String>::new()
    );

    // A new extension page the lists do not decide, and a decided one gone.
    let gone = pages.pop().unwrap();
    pages.push("extensions/new/overview");
    let problems = site_coverage(&sources, &navigation(&pages));
    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(
        problems
            .iter()
            .any(|problem| problem.starts_with("extensions/new/overview"))
    );
    assert!(problems.iter().any(|problem| problem.starts_with(gone)));

    // A navigation with no extension page at all is a restructure, not a pass.
    let problems = site_coverage(&sources, &navigation(&["specification/x"]));
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("lists no page"))
    );
}
