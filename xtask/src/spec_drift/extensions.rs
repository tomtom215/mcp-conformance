// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The extension registries' page set, quotes, census and anchors (ADR-0023).
//!
//! An official extension is not published under the specification's dated
//! directories, so nothing above reads it: the site lists each one under
//! `extensions/` and defers to its own repository (`modelcontextprotocol/ext-*`)
//! for the normative text. `registry/2026-07-28/extensions/sources.json` names,
//! per extension, the site page that introduces it, where its text is fetched,
//! and its own in-scope and out-of-scope pages with the census of each; and it
//! gives every other page the site lists under `extensions/` with the reason it
//! has no entries. This module holds those lists to the same rules a revision's
//! are held to — in both directions against the site's navigation, every quote
//! verified, every keyword counted and entered, every anchor resolved — and adds
//! one more: an extension's entries and its listed pages must agree, so a quote
//! citing a page nobody listed, or a listed page nobody cites, fails.
//!
//! [`structure`] is the offline half `cargo xtask gates` runs (through the
//! page-set gate); [`verify`] is the network half `cargo xtask spec-drift` runs.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::requirement::{Registry, Requirement};
use serde::Deserialize;

use super::census::{self, Family};
use super::quote::{normalize, quote_present};

/// The committed extension page set, relative to the workspace root.
pub(super) const SOURCES: &str =
    "crates/mcp-conformance-core/registry/2026-07-28/extensions/sources.json";

/// The revision the extensions here attach to.
pub(super) const REVISION: &str = "2026-07-28";

/// The site navigation's prefix for extension pages.
const SITE_PREFIX: &str = "extensions/";

/// `sources.json` for the extensions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExtensionSources {
    #[serde(rename = "_policy")]
    #[allow(dead_code)]
    policy: String,
    /// Extension identifier → where it is published and which of its pages
    /// are entered.
    pub(super) extensions: BTreeMap<String, Extension>,
    /// Every other page the site lists under `extensions/`, with the reason.
    pub(super) site_out_of_scope: BTreeMap<String, String>,
}

/// One extension's publication and page set.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Extension {
    /// The site page that introduces the extension (`extensions/tasks/overview`).
    pub(super) site_page: String,
    /// Where the extension's specification files are fetched from: a prefix
    /// each `in_scope` file is appended to.
    text: String,
    /// Page (as `source.section` cites it) → its file under `text`.
    pub(super) in_scope: BTreeMap<String, String>,
    /// The extension's other specification files, by their path under
    /// `text`, with the reason each is out.
    pub(super) out_of_scope: BTreeMap<String, String>,
    #[serde(default)]
    must_census: BTreeMap<String, u32>,
    #[serde(default)]
    should_census: BTreeMap<String, u32>,
    #[serde(default)]
    may_census: BTreeMap<String, u32>,
}

impl Extension {
    const fn censuses(&self) -> [(Family, &BTreeMap<String, u32>); 3] {
        [
            (Family::Must, &self.must_census),
            (Family::Should, &self.should_census),
            (Family::May, &self.may_census),
        ]
    }
}

/// Reads the committed extension page set.
pub(super) fn load() -> Result<ExtensionSources, String> {
    let path = crate::workspace_root().join(SOURCES);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} is not valid: {error}", path.display()))
}

/// The page an entry cites: its section up to the anchor.
fn page_of(requirement: &Requirement) -> &str {
    requirement
        .source
        .section
        .split('#')
        .next()
        .unwrap_or_default()
}

/// What can be checked offline: every list well-formed, every exclusion
/// reasoned, every census keyed by exactly the in-scope pages, and the entries
/// of `registry` agreeing with the lists in both directions — each extension
/// with entries is listed, each listed one has entries, and each entry cites an
/// in-scope page of its own extension, every one of which some entry cites.
pub(super) fn structure(sources: &ExtensionSources, registry: &Registry) -> Vec<String> {
    let mut problems = Vec::new();
    let mut site_pages = BTreeSet::new();
    for (id, extension) in &sources.extensions {
        if !site_pages.insert(extension.site_page.as_str())
            || sources.site_out_of_scope.contains_key(&extension.site_page)
        {
            problems.push(format!(
                "{id}: its site page {} is listed twice; each site page belongs to one \
                 extension or to `site_out_of_scope`",
                extension.site_page
            ));
        }
        for page in extension.in_scope.keys() {
            if extension.out_of_scope.contains_key(page) {
                problems.push(format!(
                    "{id}/{page}: in both `in_scope` and `out_of_scope`"
                ));
            }
            for (family, census) in extension.censuses() {
                if !census.contains_key(page) {
                    problems.push(format!(
                        "{id}/{page}: in scope with no `{}` entry",
                        family.field()
                    ));
                }
            }
        }
        for (family, census) in extension.censuses() {
            for page in census
                .keys()
                .filter(|page| !extension.in_scope.contains_key(*page))
            {
                problems.push(format!(
                    "{id}/{page}: has a `{}` entry but is not in scope",
                    family.field()
                ));
            }
        }
        for (page, reason) in &extension.out_of_scope {
            if reason.trim().is_empty() {
                problems.push(format!("{id}/{page}: `out_of_scope` without a reason"));
            }
        }
    }
    for (page, reason) in &sources.site_out_of_scope {
        if reason.trim().is_empty() || !page.starts_with(SITE_PREFIX) {
            problems.push(format!(
                "{page}: `site_out_of_scope` needs an `{SITE_PREFIX}` page and a reason"
            ));
        }
    }
    problems.extend(agreement(sources, registry));
    problems
}

/// Entries and lists, both directions (see [`structure`]).
fn agreement(sources: &ExtensionSources, registry: &Registry) -> Vec<String> {
    let mut problems = Vec::new();
    let mut cited: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for requirement in registry.extension_requirements() {
        let id = requirement.extension.as_deref().unwrap_or_default();
        let page = page_of(requirement);
        match sources.extensions.get(id) {
            None => problems.push(format!(
                "{}: names extension {id}, which {SOURCES} does not list",
                requirement.id
            )),
            Some(extension) if !extension.in_scope.contains_key(page) => problems.push(format!(
                "{}: cites {page}, which is not an in-scope page of {id}",
                requirement.id
            )),
            Some(_) => {}
        }
        cited.entry(id).or_default().insert(page);
    }
    for (id, extension) in &sources.extensions {
        for page in extension.in_scope.keys() {
            if !cited
                .get(id.as_str())
                .is_some_and(|pages| pages.contains(page.as_str()))
            {
                problems.push(format!(
                    "{id}/{page}: listed in scope, but no entry of {id} cites it"
                ));
            }
        }
    }
    problems
}

/// What the network half found, in the parent's terms.
#[derive(Debug, Default)]
pub(super) struct Found {
    pub(super) quotes: u32,
    pub(super) drifted: u32,
    pub(super) broken_anchors: u32,
    pub(super) unlisted_pages: u32,
    pub(super) census: u32,
}

/// The network half: the page set against the site navigation, then each
/// extension's quotes, census and anchors against its published text.
///
/// # Errors
///
/// When the lists cannot be read, or a page cannot be fetched: an unverified
/// page is not a verified page.
pub(super) fn verify(navigation: &str, registry: &Registry) -> Result<Found, ()> {
    let sources =
        load().map_err(|message| eprintln!("xtask: spec-drift — extensions: {message}"))?;
    let mut found = Found::default();
    let mut problems = structure(&sources, registry);
    problems.extend(site_coverage(&sources, navigation));
    for problem in &problems {
        eprintln!("xtask: spec-drift — extensions: {problem}");
    }
    found.unlisted_pages = u32::try_from(problems.len()).unwrap_or(u32::MAX);
    for (id, extension) in &sources.extensions {
        let entries: Vec<&Requirement> = registry
            .extension_requirements()
            .iter()
            .filter(|requirement| requirement.extension.as_deref() == Some(id.as_str()))
            .collect();
        verify_extension(id, extension, &entries, &mut found)?;
    }
    Ok(found)
}

/// Every page the site lists under `extensions/` is an extension's site page
/// or reasoned out, and every page either list names is one the site lists.
fn site_coverage(sources: &ExtensionSources, navigation: &str) -> Vec<String> {
    let Ok(document) = serde_json::from_str::<serde_json::Value>(navigation) else {
        return vec!["the site navigation is not JSON".to_owned()];
    };
    let mut listed = Vec::new();
    super::pages::navigation_pages(&document, &mut listed);
    let published: BTreeSet<&str> = listed
        .into_iter()
        .filter(|page| page.starts_with(SITE_PREFIX))
        .collect();
    let named: BTreeSet<&str> = sources
        .extensions
        .values()
        .map(|extension| extension.site_page.as_str())
        .chain(sources.site_out_of_scope.keys().map(String::as_str))
        .collect();
    let mut problems: Vec<String> = published
        .difference(&named)
        .map(|page| {
            format!(
                "{page}: the site publishes it under `{SITE_PREFIX}`, and {SOURCES} neither \
                 enters its extension nor gives a reason to leave it out"
            )
        })
        .collect();
    problems.extend(
        named.difference(&published).map(|page| {
            format!("{page}: listed in {SOURCES}, but the site publishes no such page")
        }),
    );
    if published.is_empty() {
        problems.push(format!(
            "the site navigation lists no page under `{SITE_PREFIX}`; if it was restructured, \
             teach `spec_drift::extensions` to read it again"
        ));
    }
    problems
}

/// One extension's quotes, census and anchors, added to `found`.
fn verify_extension(
    id: &str,
    extension: &Extension,
    entries: &[&Requirement],
    found: &mut Found,
) -> Result<(), ()> {
    let mut live: BTreeMap<String, census::Tally> = BTreeMap::new();
    let mut unentered = Vec::new();
    for (page, file) in &extension.in_scope {
        let url = format!("{}{file}", extension.text);
        let text = super::fetch(&url)
            .map_err(|message| eprintln!("xtask: spec-drift — cannot fetch {url}: {message}"))?;
        live.insert(page.clone(), census::count(&text));
        let on_page: Vec<&&Requirement> = entries.iter().filter(|r| page_of(r) == page).collect();
        let quotes = on_page.iter().map(|r| r.source.quote.as_str());
        for missing in census::uncovered(&census::prose(&text), quotes) {
            unentered.push(format!("{id}/{page}: {missing}"));
        }
        let normalized = normalize(&text);
        let mut drifted = 0u32;
        for requirement in &on_page {
            if !quote_present(&normalized, &requirement.source.quote) {
                eprintln!(
                    "xtask: spec-drift — {}: quote no longer found on {id}/{page}:\n  {:?}",
                    requirement.id, requirement.source.quote
                );
                drifted += 1;
            }
        }
        eprintln!(
            "xtask: spec-drift — {id}/{page}: {} quote(s), {drifted} drifted (content {})",
            on_page.len(),
            super::fingerprint(&text)
        );
        found.quotes += u32::try_from(on_page.len()).unwrap_or(u32::MAX);
        found.drifted += drifted;
        found.broken_anchors += broken_anchors(&on_page)?;
    }
    // The out-of-scope files are fetched too, so a reason cannot outlive its file.
    for file in extension.out_of_scope.keys() {
        let url = format!("{}{file}", extension.text);
        if super::fetch(&url).is_err() {
            eprintln!(
                "xtask: spec-drift — {id}/{file}: listed out of scope, but {url} serves nothing"
            );
            found.unlisted_pages += 1;
        }
    }
    found.census += report_census(id, extension, &live, &unentered);
    Ok(())
}

/// The anchors an extension's entries cite on one page, against the heading ids
/// of the page as the extension publishes it. A page none of whose entries
/// carries an anchor is not fetched: it has nothing to resolve.
fn broken_anchors(entries: &[&&Requirement]) -> Result<u32, ()> {
    let anchored: Vec<&&&Requirement> = entries
        .iter()
        .filter(|requirement| requirement.source.section.contains('#'))
        .collect();
    let Some(first) = anchored.first() else {
        return Ok(0);
    };
    let url = first.url(REVISION.parse().map_err(|_| ())?);
    let page = url.split_once('#').map_or(url.as_str(), |(page, _)| page);
    let html = super::fetch(page)
        .map_err(|message| eprintln!("xtask: spec-drift — cannot fetch {page}: {message}"))?;
    let offered = super::anchors::heading_ids(&html);
    let mut broken = 0u32;
    for requirement in anchored {
        if let Some((_, anchor)) = requirement.source.section.split_once('#')
            && !offered.contains(anchor)
        {
            eprintln!(
                "xtask: spec-drift — {}: {page} publishes no anchor #{anchor}",
                requirement.id
            );
            broken += 1;
        }
    }
    Ok(broken)
}

/// [`census::reconcile`] for each family, then every unentered keyword; prints
/// each problem and returns how many there were.
fn report_census(
    id: &str,
    extension: &Extension,
    live: &BTreeMap<String, census::Tally>,
    unentered: &[String],
) -> u32 {
    let mut problems: Vec<String> = unentered.to_vec();
    let mut totals = Vec::new();
    for (family, committed) in extension.censuses() {
        match census::reconcile(id, family, committed, live) {
            Ok(total) => totals.push(format!("{total} {}", family.label())),
            Err(found) => problems.extend(found),
        }
    }
    for problem in &problems {
        eprintln!("xtask: spec-drift — {problem}");
    }
    if problems.is_empty() {
        eprintln!(
            "xtask: spec-drift — {id}: {} keyword(s) across {} page(s), matching the \
             committed census; every MUST-family and SHOULD-family one inside an entry's quote",
            totals.join(", "),
            live.len()
        );
    }
    u32::try_from(problems.len()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
