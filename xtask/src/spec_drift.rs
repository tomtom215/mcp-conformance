// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The spec-drift gate: every registry quote re-verified against the living
//! spec text (ADR-0010).
//!
//! Registry entries carry verbatim quotes with `source.section` pages. The
//! spec repository is living: a silently edited clause would leave the
//! registry quoting text that no longer exists — round two verified every
//! quote with a `/tmp` script that died with its session, which is exactly
//! the claims-expire failure mode this gate exists to kill. It fetches each
//! in-scope page from the published spec source and verifies every quote is
//! present under the normalization `SourceRef::quote` itself documents
//! (whitespace collapse; bullet/numbered lists flattened with `"; "` joins).
//!
//! Network use puts this beside `conformance` on the orchestration side of
//! the boundary: it runs in the weekly scheduled job and on demand, never
//! inside `cargo test`. A fetch failure fails the gate — an unverified page
//! is not a verified page. The in-scope page set is the committed
//! `sources.json` beside the registry; the gate enforces, both directions,
//! that the listed set and the set of pages entries actually cite are
//! identical, so the explicit list can never drift from the registry it
//! describes.
//!
//! Quotes are only half of it. Verifying the ones the registry *has* cannot see
//! a clause it never had, so a page could gain a MUST with every committed quote
//! still verifying and the run still green. [`census`] closes that direction:
//! each page's MUST-family keyword count is committed beside the page list and
//! recounted here. Together the two say the registry still quotes the spec
//! accurately *and* still quotes all of it.
//!
//! A third check reads the page as the site publishes it: every `source.section`
//! anchor must be a heading id there, because reports link failing clauses to it
//! ([`anchors`]).
//!
//! The census can only count the pages it is told about. A fourth check reads
//! the site's navigation and holds `sources.json` to it: every page a revision
//! publishes is in exactly one of `in_scope` / `out_of_scope`, and every
//! exclusion carries its reason ([`pages`]) — so a page can no longer sit in
//! neither list, where nothing above would ever read it.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use mcp_conformance_core::requirement::{Registry, RegistrySet, Requirement};
use serde::Deserialize;

mod anchors;
mod census;
pub(crate) mod pages;

/// The revisions this gate verifies, each against its own published pages.
///
/// `2026-07-28` is built area by area, so its `sources.json` lists only the pages
/// whose areas have landed — the set-agreement check below then holds for it exactly
/// as it does for the complete revision: every listed page is cited, every cited page
/// is listed. The gate reads the registry *set*, so a revision appears here only once
/// its entries are embedded.
const REVISIONS: &[&str] = &["2025-11-25", "2026-07-28"];

/// The committed in-scope page set for a revision, relative to the workspace root.
fn sources_path(revision: &str) -> String {
    format!("crates/mcp-conformance-core/registry/{revision}/sources.json")
}

/// Where a revision's published spec text lives, per page file.
///
/// Revision-scoped rather than fixed: the spec repo publishes each revision
/// under its own dated directory, so a quote is only ever checked against the
/// text of the revision that carries it. That distinction became load-bearing
/// on 2026-07-28, when `2026-07-28` shipped alongside `2025-11-25` (register
/// 1.5h) — before that there was only one published revision to point at.
fn raw_base(revision: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/{revision}"
    )
}

/// The committed in-scope/out-of-scope page sets.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    #[serde(rename = "_policy")]
    #[allow(dead_code)]
    policy: String,
    /// Page path (as registry `source.section` prefixes cite it) → the
    /// spec-repo file it is published from.
    in_scope: BTreeMap<String, String>,
    /// Pages of the revision deliberately out of scope, with reasons.
    out_of_scope: BTreeMap<String, String>,
    /// Why the census exists and how to re-decide it, beside the data.
    #[serde(rename = "_must_census_policy")]
    #[allow(dead_code)]
    must_census_policy: String,
    /// Page → how many MUST-family clauses its prose carries, so rule 1's
    /// "no exceptions" is a gate rather than a claim (`census`). Defaults to
    /// empty, which the reconciliation then reports page by page — a revision
    /// mid-extraction is not required to have censused every page yet.
    #[serde(default)]
    must_census: BTreeMap<String, u32>,
}

pub(crate) fn run() -> ExitCode {
    let set = match RegistrySet::builtin() {
        Ok(set) => set,
        Err(error) => {
            eprintln!("xtask: spec-drift — embedded registry set failed to load: {error}");
            return ExitCode::FAILURE;
        }
    };
    let Ok(navigation) = pages::navigation() else {
        return ExitCode::FAILURE;
    };
    let mut drifted = 0u32;
    let mut checked = 0u32;
    let mut broken = 0u32;
    let mut unlisted = 0u32;
    let mut verified: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for revision in REVISIONS {
        let Ok(parsed) = revision.parse() else {
            eprintln!("xtask: spec-drift — {revision} is not a protocol revision");
            return ExitCode::FAILURE;
        };
        // A revision the set does not describe has no embedded entries to verify.
        let Some(registry) = set.registry(parsed) else {
            eprintln!("xtask: spec-drift — {revision}: not described by this build, skipped");
            skipped.push(revision);
            continue;
        };
        verified.push(revision);
        match verify_revision(revision, &registry, &navigation) {
            Ok(count) => {
                checked += count.quotes;
                drifted += count.drifted;
                broken += count.broken_anchors;
                unlisted += count.unlisted_pages;
            }
            Err(()) => return ExitCode::FAILURE,
        }
    }

    if !explain_failures(drifted, broken, unlisted) {
        return ExitCode::FAILURE;
    }
    // The count is of revisions *verified*, not of revisions looped over. It
    // used to be `REVISIONS.len()`, so a build without the draft feature
    // announced "140 quote(s) across 2 revision(s)" having read one of them —
    // a gate overstating its own coverage, which is the failure this gate
    // exists to catch in other people's documents.
    eprintln!(
        "xtask: spec-drift — {checked} quote(s) and their anchors across {} revision(s) \
         verified against the published text{}",
        verified.len(),
        if skipped.is_empty() {
            String::new()
        } else {
            format!(
                "; {} NOT verified, this build does not describe {}",
                skipped.len(),
                skipped.join(", ")
            )
        }
    );
    ExitCode::SUCCESS
}

/// Says what to do about each kind of failure found; `true` when there were none.
fn explain_failures(drifted: u32, broken: u32, unlisted: u32) -> bool {
    if drifted > 0 {
        eprintln!(
            "xtask: spec-drift — {drifted} quote(s) drifted from the published text. \
             Re-read each clause: if the requirement changed, update the entry (and \
             its checks or exclusion); if only the wording moved, refresh the quote."
        );
    }
    if broken > 0 {
        eprintln!(
            "xtask: spec-drift — {broken} section anchor(s) the published pages do not \
             offer. Point each `source.section` at the heading id the page publishes \
             (the nearest `####` or shallower heading: deeper ones get no anchor)."
        );
    }
    if unlisted > 0 {
        eprintln!(
            "xtask: spec-drift — {unlisted} page-set problem(s) in sources.json. Every page \
             the revision publishes belongs in exactly one of `in_scope` (with its source \
             file, its entries and its census) or `out_of_scope` (with the reason)."
        );
    }
    drifted == 0 && broken == 0 && unlisted == 0
}

/// What one revision's verification found.
struct Counts {
    /// Quotes checked.
    quotes: u32,
    /// Quotes no longer present in the published source.
    drifted: u32,
    /// Requirements whose section anchor the published page does not offer.
    broken_anchors: u32,
    /// Ways `sources.json` disagrees with the pages the revision publishes.
    unlisted_pages: u32,
}

/// Verifies one revision's quotes against its own published pages, and each
/// quote's anchor against the page as the site renders it.
///
/// Returns the [`Counts`], or `Err(())` when the revision could not be verified at
/// all — an unreadable sources file, a page/registry disagreement, or a failed
/// fetch. An unverified page is not a verified page.
fn verify_revision(revision: &str, registry: &Registry, navigation: &str) -> Result<Counts, ()> {
    let sources = load_sources(revision).map_err(|message| {
        eprintln!("xtask: spec-drift — {revision}: {message}");
    })?;
    // First, because everything after it reads only the pages `in_scope` names:
    // a page in neither list would otherwise pass every check this gate runs.
    let unlisted_pages = pages::report(revision, &sources, navigation);
    let by_page = requirements_by_page(registry);
    if !sets_agree(&sources, &by_page) {
        return Err(());
    }

    let mut checked = 0u32;
    let mut drifted = 0u32;
    let mut broken = 0u32;
    let mut live_census: BTreeMap<String, u32> = BTreeMap::new();
    for (page, requirements) in &by_page {
        let url = format!("{}/{}", raw_base(revision), sources.in_scope[page]);
        let text = fetch(&url).map_err(|message| {
            eprintln!("xtask: spec-drift — cannot fetch {url}: {message}");
        })?;
        live_census.insert(page.clone(), census::count(&text));
        let normalized = normalize(&text);
        let mut page_drifted = 0u32;
        for requirement in requirements {
            if !quote_present(&normalized, &requirement.source.quote) {
                eprintln!(
                    "xtask: spec-drift — {}: quote no longer found on {revision}/{page}:\n  {:?}",
                    requirement.id, requirement.source.quote
                );
                page_drifted += 1;
            }
        }
        eprintln!(
            "xtask: spec-drift — {revision}/{page}: {} quote(s), {page_drifted} drifted (content {})",
            requirements.len(),
            fingerprint(&text)
        );
        checked += u32::try_from(requirements.len()).unwrap_or(u32::MAX);
        drifted += page_drifted;
        broken += anchors::broken(registry, requirements)?;
    }

    // The other direction: the quotes above prove what the registry *has* is
    // still accurate; this proves the pages have gained nothing it lacks.
    match census::reconcile(revision, &sources.must_census, &live_census) {
        Ok(total) => eprintln!(
            "xtask: spec-drift — {revision}: {total} MUST-family clause(s) across \
             {} in-scope page(s), matching the committed census",
            live_census.len()
        ),
        Err(problems) => {
            for problem in &problems {
                eprintln!("xtask: spec-drift — {problem}");
            }
            drifted += u32::try_from(problems.len()).unwrap_or(u32::MAX);
        }
    }
    Ok(Counts {
        quotes: checked,
        drifted,
        broken_anchors: broken,
        unlisted_pages,
    })
}

/// Registry requirements grouped by the page their `source.section` cites.
fn requirements_by_page(registry: &Registry) -> BTreeMap<String, Vec<&Requirement>> {
    let mut by_page: BTreeMap<String, Vec<&Requirement>> = BTreeMap::new();
    for requirement in registry.requirements() {
        let page = requirement
            .source
            .section
            .split('#')
            .next()
            .unwrap_or_default()
            .to_owned();
        by_page.entry(page).or_default().push(requirement);
    }
    by_page
}

/// Both directions: every cited page is listed in-scope, every listed page
/// is cited. A mismatch is a registry/sources drift, reported per page.
fn sets_agree(sources: &Sources, by_page: &BTreeMap<String, Vec<&Requirement>>) -> bool {
    let listed: BTreeSet<&String> = sources.in_scope.keys().collect();
    let cited: BTreeSet<&String> = by_page.keys().collect();
    for page in cited.difference(&listed) {
        eprintln!(
            "xtask: spec-drift — registry entries cite {page}, which sources.json \
             does not list as in-scope; add it (with its source file) or fix the entries"
        );
    }
    for page in listed.difference(&cited) {
        eprintln!(
            "xtask: spec-drift — sources.json lists {page} in-scope, but no registry \
             entry cites it; remove the row or add the missing entries"
        );
    }
    listed == cited
}

fn load_sources(revision: &str) -> Result<Sources, String> {
    let path = crate::workspace_root().join(sources_path(revision));
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} is not valid: {error}", path.display()))
}

/// Fetches one URL via curl — a checked tool dependency CI runners already
/// carry; two network calls a week do not justify an HTTP client in xtask.
fn fetch(url: &str) -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args(["-sSf", "--max-time", "30", url])
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "curl exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "page is not UTF-8".to_owned())
}

/// A short content fingerprint so a drift report names exactly what was
/// checked. `git hash-object` because git is already a hard dependency of
/// the docs-links gate; failure degrades to "unfingerprinted", never a
/// gate verdict.
fn fingerprint(text: &str) -> String {
    use std::io::Write as _;
    let spawn = std::process::Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = spawn else {
        return "unfingerprinted".to_owned();
    };
    if let Some(stdin) = child.stdin.as_mut()
        && stdin.write_all(text.as_bytes()).is_err()
    {
        return "unfingerprinted".to_owned();
    }
    child.wait_with_output().map_or_else(
        |_| "unfingerprinted".to_owned(),
        |output| {
            let hash = String::from_utf8_lossy(&output.stdout);
            hash.trim().chars().take(12).collect()
        },
    )
}

pub(crate) mod quote;

use quote::{normalize, quote_present};

#[cfg(test)]
mod tests;
