// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The page set: every page a revision publishes sits in exactly one of
//! `sources.json`'s two lists, and every page left out says why.
//!
//! The rest of the drift gate reads only the pages `in_scope` names, so its
//! completeness claim is exactly as complete as that list. Until 2026-10-12
//! nothing held the list to the specification: `basic/patterns/cancellation`,
//! `basic/patterns/progress` and `basic/patterns/index` were published with
//! `2026-07-28` and appeared in neither list, so their MUSTs were neither
//! entered nor excluded, and the census — which counts listed pages — could
//! not see them. The quote check was green; it had nothing to check.
//!
//! **What the published page set is.** The site's navigation
//! (`docs/docs.json`), read from the same branch and host the quotes are, rather
//! than a directory listing: it is what a reader of the specification can
//! reach, it needs no API token (the GitHub tree API does, past sixty calls an
//! hour), and at the time of writing it lists exactly the `.mdx` files under
//! both revisions' directories.
//!
//! **Two halves.** [`verify`] runs inside `cargo xtask spec-drift` and compares
//! the lists against the live navigation in both directions. [`gate`] is the
//! offline half `cargo xtask gates` runs: the tree carries no copy of the page
//! set beyond `sources.json` itself, so offline the lists can only be held to
//! their own shape — disjoint, every exclusion reasoned, every in-scope page
//! naming its own source file and censused. A page dropped from *both* lists is
//! visible offline only to the test below that pins each final revision's
//! navigation; the weekly drift run is what catches it against the live site.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{REVISIONS, Sources, load_sources};

/// The site navigation: every page each revision publishes, by path.
const NAVIGATION_URL: &str = "https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/docs.json";

/// The site navigation, fetched once per run since one document lists every
/// revision's pages. A failed fetch is reported and fails the run: an
/// unverified page set is not a verified one.
pub(super) fn navigation() -> Result<String, ()> {
    super::fetch(NAVIGATION_URL).map_err(|message| {
        eprintln!("xtask: spec-drift — cannot fetch {NAVIGATION_URL}: {message}");
    })
}

/// One name per page. A directory's index is cited both ways — `2025-11-25`
/// lists `basic`, `2026-07-28` lists `basic/index`, and the site serves both at
/// the directory — so the comparison folds the suffix away. The revision's own
/// root page stays `index`.
pub(super) fn canonical(page: &str) -> &str {
    page.strip_suffix("/index").unwrap_or(page)
}

/// Runs [`verify`] for one revision and prints what it found, a problem per
/// line or one line saying the lists hold; returns how many problems there were.
pub(super) fn report(revision: &str, sources: &Sources, navigation: &str) -> u32 {
    let problems = verify(revision, sources, navigation);
    for problem in &problems {
        eprintln!("xtask: spec-drift — {problem}");
    }
    if problems.is_empty() {
        eprintln!(
            "xtask: spec-drift — {revision}: the site publishes {} page(s), each in exactly \
             one of `in_scope` / `out_of_scope`",
            sources.in_scope.len() + sources.out_of_scope.len()
        );
    }
    u32::try_from(problems.len()).unwrap_or(u32::MAX)
}

/// Every problem `sources.json` has against the published navigation, plus
/// the offline [`structure`] problems, each naming the revision and page.
pub(super) fn verify(revision: &str, sources: &Sources, navigation: &str) -> Vec<String> {
    let mut problems = structure(revision, sources);
    match published(navigation, revision) {
        Ok(pages) => problems.extend(coverage(revision, sources, &pages)),
        Err(message) => problems.push(format!("{revision}: {message}")),
    }
    problems
}

/// The pages the navigation lists under `specification/{revision}/`, canonical.
///
/// # Errors
///
/// When the document does not parse, or lists no page for the revision — a
/// navigation restructured past what this reads must fail, not verify an empty
/// set against itself.
pub(super) fn published(navigation: &str, revision: &str) -> Result<BTreeSet<String>, String> {
    let document: Value = serde_json::from_str(navigation)
        .map_err(|error| format!("the site navigation is not JSON: {error}"))?;
    let mut listed = Vec::new();
    navigation_pages(&document, &mut listed);
    let prefix = format!("specification/{revision}/");
    let pages: BTreeSet<String> = listed
        .into_iter()
        .filter_map(|page| page.strip_prefix(&prefix))
        .map(|page| canonical(page).to_owned())
        .collect();
    if pages.is_empty() {
        return Err(format!(
            "the site navigation lists no page under `{prefix}`; if it was restructured, \
             teach `spec_drift::pages` to read it again"
        ));
    }
    Ok(pages)
}

/// Every string in a `pages` array, at any depth: the navigation nests groups
/// inside tabs inside versions, and a page is a string wherever it appears.
pub(super) fn navigation_pages<'v>(value: &'v Value, out: &mut Vec<&'v str>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "pages"
                    && let Some(items) = child.as_array()
                {
                    out.extend(items.iter().filter_map(Value::as_str));
                }
                navigation_pages(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                navigation_pages(item, out);
            }
        }
        _ => {}
    }
}

/// Both directions against the published set: a page in neither list, and a
/// listed page the revision does not publish.
fn coverage(revision: &str, sources: &Sources, published: &BTreeSet<String>) -> Vec<String> {
    let listed: BTreeSet<&str> = sources
        .in_scope
        .keys()
        .chain(sources.out_of_scope.keys())
        .map(|page| canonical(page))
        .collect();
    let mut problems = Vec::new();
    for page in published {
        if !listed.contains(page.as_str()) {
            problems.push(format!(
                "{revision}/{page}: published, but in neither `in_scope` nor `out_of_scope`. \
                 Read the page and decide: enter its clauses, or exclude it with the reason."
            ));
        }
    }
    for page in listed {
        if !published.contains(page) {
            problems.push(format!(
                "{revision}/{page}: listed in sources.json, but the revision publishes no \
                 such page. Fix the name, or retire the row."
            ));
        }
    }
    problems
}

/// What can be checked without the network: the two lists are disjoint and
/// spell each page once, every exclusion has a reason, every in-scope page
/// names its own source file, and the census covers exactly the in-scope pages.
pub(super) fn structure(revision: &str, sources: &Sources) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    let lists = [
        ("in_scope", &sources.in_scope),
        ("out_of_scope", &sources.out_of_scope),
    ];
    for (list, pages) in lists {
        for page in pages.keys() {
            if let Some(first) = seen.insert(canonical(page), list) {
                problems.push(format!(
                    "{revision}/{page}: listed in `{list}` and already in `{first}`; a page \
                     belongs in exactly one list, under one name"
                ));
            }
        }
    }
    for (page, reason) in &sources.out_of_scope {
        if reason.trim().is_empty() {
            problems.push(format!(
                "{revision}/{page}: `out_of_scope` without a reason; say why the page is \
                 excluded, or it reads as work nobody decided"
            ));
        }
    }
    for (page, file) in &sources.in_scope {
        if *file != format!("{page}.mdx") && *file != format!("{page}/index.mdx") {
            problems.push(format!(
                "{revision}/{page}: `in_scope` names {file:?}, which is not this page's source"
            ));
        }
        for (family, census) in sources.censuses() {
            if !census.contains_key(page) {
                problems.push(format!(
                    "{revision}/{page}: in scope with no `{}` entry; count its {} keywords",
                    family.field(),
                    family.label()
                ));
            }
        }
    }
    for (family, census) in sources.censuses() {
        for page in census.keys() {
            if !sources.in_scope.contains_key(page) {
                problems.push(format!(
                    "{revision}/{page}: has a `{}` entry but is not in scope",
                    family.field()
                ));
            }
        }
    }
    problems
}

/// The offline half, run by `cargo xtask gates`: every revision's lists are
/// well-formed by [`structure`], and so are the draft tracking skeleton's —
/// it is the next revision's `sources.json` in waiting, so it is held to the
/// same shape before it ever judges anything. `true` when they are.
pub(crate) fn gate() -> bool {
    let mut ok = true;
    let mut pages = 0usize;
    for revision in REVISIONS.iter().chain([&super::draft::DRAFT]) {
        let sources = match load_sources(revision) {
            Ok(sources) => sources,
            Err(message) => {
                eprintln!("xtask: page-set — {revision}: {message}");
                ok = false;
                continue;
            }
        };
        pages += sources.in_scope.len() + sources.out_of_scope.len();
        for problem in structure(revision, &sources) {
            eprintln!("xtask: page-set — {problem}");
            ok = false;
        }
    }
    ok &= extension_gate();
    if ok {
        eprintln!(
            "xtask: page-set — {pages} page(s) across {} revision(s) and the draft skeleton, \
             each in exactly one list, every exclusion reasoned (the published set itself is \
             checked by `cargo xtask spec-drift`, the draft's by `spec-drift --draft`)",
            REVISIONS.len()
        );
    }
    ok
}

/// The offline half for the extension registries (ADR-0023): their page set's
/// shape, and its agreement with the entries that cite it.
fn extension_gate() -> bool {
    let sources = match super::extensions::load() {
        Ok(sources) => sources,
        Err(message) => {
            eprintln!("xtask: page-set — extensions: {message}");
            return false;
        }
    };
    let registry = mcp_conformance_core::requirement::RegistrySet::builtin()
        .ok()
        .and_then(|set| set.registry(super::extensions::REVISION.parse().ok()?));
    let Some(registry) = registry else {
        eprintln!(
            "xtask: page-set — the embedded set does not describe {}",
            super::extensions::REVISION
        );
        return false;
    };
    let problems = super::extensions::structure(&sources, &registry);
    for problem in &problems {
        eprintln!("xtask: page-set — extensions: {problem}");
    }
    if problems.is_empty() {
        eprintln!(
            "xtask: page-set — {} extension(s) attached to {}, each page in one list and \
             cited by its entries, {} other extension page(s) of the site reasoned out",
            sources.extensions.len(),
            super::extensions::REVISION,
            sources.site_out_of_scope.len()
        );
    }
    problems.is_empty()
}

#[cfg(test)]
mod tests;
