// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask spec-drift --draft` — what the specification's `draft/`
//! revision has changed since the newest release, read before it ships.
//!
//! The release gate ([`super::run`]) holds each dated revision to its own
//! published text, and a dated revision does not move. The *next* one does:
//! upstream edits `docs/specification/draft/` between releases, and until this
//! task existed the only record of what it had changed was a sentence in a
//! review (TRAN-081's safe-integer wording). This reads the draft the way the
//! release gate reads a release — same navigation, same host, same
//! normalization — and reports three things:
//!
//! 1. **The page set**: every page the draft publishes against the committed
//!    skeleton's `in_scope` / `out_of_scope` (`registry/draft/sources.json`).
//! 2. **The census**: each in-scope draft page's keyword counts against the
//!    skeleton's, and every MUST-family or SHOULD-family keyword on a draft page
//!    that no released quote citing the page covers — a clause the next
//!    revision adds or rewords.
//! 3. **The quotes**: for every entry of the newest released registry,
//!    whether its sentence is still on the corresponding draft page — present,
//!    changed (with the draft's wording), removed, or on a page the draft no
//!    longer publishes. A change already recorded in
//!    `registry/draft/post-release.json` is marked so.
//!
//! **Informational.** It exits 0 with its report whatever it finds; only a
//! draft that cannot be read (the navigation, or a page it lists) or a
//! committed skeleton that cannot be loaded fails it. The report is Markdown,
//! on stdout and appended to `$GITHUB_STEP_SUMMARY` when that is set.
//!
//! **Why the skeleton cannot reach a verdict.** It lives at
//! `crates/mcp-conformance-core/registry/draft/`, beside the revisions it will
//! join, but the core crate embeds registry documents by an explicit
//! `include_str!` list per revision and lists none from `draft/`, `draft` is
//! not a `ProtocolRevision` (so no `--revision` can name it), and the crate's
//! manifest excludes the directory from the published package. It holds no
//! requirement entries at all — only the page lists, the census, and the
//! post-release record — so there is nothing in it a registry loader could
//! read even if one were pointed at it.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::process::ExitCode;

use mcp_conformance_core::requirement::{Registry, RegistrySet, Requirement};
use serde::Deserialize;

use super::census::{self, Tally};
use super::pages::{self, canonical};
use super::quote::{normalize, quote_present};
use super::{Sources, fetch, load_sources, raw_base, requirements_by_page};

mod compare;
mod render;

use compare::Status;

/// The spec repository's directory, and this skeleton's, for the next revision.
pub(super) const DRAFT: &str = "draft";

/// The released revision whose quotes are carried forward and looked for in
/// the draft: the newest one. Moves when a revision ships, with the skeleton.
pub(super) const BASE: &str = "2026-07-28";

/// Where post-release changes to the base revision's clauses are recorded.
const POST_RELEASE: &str = "crates/mcp-conformance-core/registry/draft/post-release.json";

/// The committed record of clauses the draft has changed since [`BASE`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostRelease {
    #[serde(rename = "_policy")]
    #[allow(dead_code)]
    policy: String,
    /// The released revision the record is against; must be [`BASE`].
    base: String,
    /// One row per changed clause.
    changes: Vec<Change>,
}

/// One clause the draft words differently from the released text.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    /// The base registry entry whose quote the draft no longer carries.
    id: String,
    /// That entry's quote, as released — must equal the registry's.
    released: String,
    /// The draft's wording, in the same quote form, verbatim on the draft page.
    draft: String,
    /// When the change was recorded (`YYYY-MM-DD`).
    #[allow(dead_code)]
    recorded: String,
    /// What the change means for the entry, and what to do when it ships.
    #[allow(dead_code)]
    reading: String,
}

/// Loads the committed post-release record.
fn load_post_release() -> Result<PostRelease, String> {
    let path = crate::workspace_root().join(POST_RELEASE);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} is not valid: {error}", path.display()))
}

/// Problems with the record that need no network: the base it names, and
/// each row's id and released quote against the embedded registry.
fn record_problems(record: &PostRelease, registry: &Registry) -> Vec<String> {
    let mut problems = Vec::new();
    if record.base != BASE {
        problems.push(format!(
            "post-release.json records changes against {}, but the draft is compared with {BASE}",
            record.base
        ));
    }
    for change in &record.changes {
        match registry
            .requirements()
            .iter()
            .find(|r| r.id.as_str() == change.id)
        {
            None => problems.push(format!(
                "{}: recorded, but {BASE} has no such entry",
                change.id
            )),
            Some(entry) if entry.source.quote != change.released => problems.push(format!(
                "{}: the recorded `released` quote is not the registry's",
                change.id
            )),
            Some(_) => {}
        }
    }
    problems
}

/// One base entry's fate in the draft.
#[derive(Debug)]
struct Finding<'r> {
    requirement: &'r Requirement,
    page: String,
    status: Status,
    /// The post-release record's row for this entry, when it has one and the
    /// recorded draft wording is on the draft page.
    recorded: bool,
}

/// Everything the report renders.
#[derive(Debug, Default)]
struct Report<'r> {
    skeleton_problems: Vec<String>,
    pages: compare::PageDelta,
    published: usize,
    fingerprints: BTreeMap<String, String>,
    base_census: BTreeMap<String, Tally>,
    live_census: BTreeMap<String, Tally>,
    census_moves: Vec<compare::CensusMove>,
    unentered: Vec<String>,
    findings: Vec<Finding<'r>>,
    /// Recorded changes whose draft wording is no longer on the page.
    stale_records: Vec<String>,
}

pub(crate) fn run() -> ExitCode {
    match collect_and_render() {
        Ok(markdown) => {
            print!("{markdown}");
            append_step_summary(&markdown);
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("xtask: spec-drift --draft — {message}");
            ExitCode::FAILURE
        }
    }
}

fn collect_and_render() -> Result<String, String> {
    let set = RegistrySet::builtin().map_err(|e| format!("registry set: {e}"))?;
    let base_revision = BASE
        .parse()
        .map_err(|_| format!("{BASE} is not a revision"))?;
    let registry = set
        .registry(base_revision)
        .ok_or_else(|| format!("this build does not describe {BASE}"))?;
    let skeleton = load_sources(DRAFT)?;
    let base_sources = load_sources(BASE)?;
    let record = load_post_release()?;
    let navigation = pages::navigation().map_err(|()| "the site navigation".to_owned())?;
    let live = pages::published(&navigation, DRAFT)?;

    let mut report = Report {
        skeleton_problems: pages::structure(DRAFT, &skeleton),
        pages: compare::page_delta(
            skeleton.in_scope.keys().chain(skeleton.out_of_scope.keys()),
            &live,
        ),
        published: live.len(),
        ..Report::default()
    };
    report
        .skeleton_problems
        .extend(record_problems(&record, &registry));

    let by_page = requirements_by_page(&registry);
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    // Every page either list cites that the draft still publishes: the
    // skeleton's in-scope pages for the census, the base's for the quotes.
    let wanted = skeleton.in_scope.iter().chain(base_sources.in_scope.iter());
    for (page, file) in wanted {
        if texts.contains_key(page) || !live.contains(canonical(page)) {
            continue;
        }
        let url = format!("{}/{file}", raw_base(DRAFT));
        let text = fetch(&url).map_err(|message| format!("cannot fetch {url}: {message}"))?;
        report
            .fingerprints
            .insert(page.clone(), super::fingerprint(&text));
        texts.insert(page.clone(), text);
    }
    for page in skeleton.in_scope.keys() {
        let Some(text) = texts.get(page) else {
            continue;
        };
        report.live_census.insert(page.clone(), census::count(text));
        let quotes = by_page
            .get(page)
            .into_iter()
            .flatten()
            .map(|r| r.source.quote.as_str());
        for missing in census::uncovered(&census::prose(text), quotes) {
            report.unentered.push(format!("{page}: {missing}"));
        }
    }
    report.census_moves = compare::census_moves(skeleton.censuses(), &report.live_census);
    report.base_census = committed_tallies(&base_sources);

    let (findings, stale) = findings(&by_page, &texts, &record);
    report.findings = findings;
    report.stale_records = stale;
    Ok(render::markdown(&report))
}

/// Every base entry's fate on its draft page, from the pages' raw text (a page
/// absent from `texts` is one the draft no longer publishes), and the ids of
/// recorded changes the draft no longer bears out: the recorded wording is not
/// on the page, or the released wording is back.
fn findings<'r>(
    by_page: &BTreeMap<String, Vec<&'r Requirement>>,
    texts: &BTreeMap<String, String>,
    record: &PostRelease,
) -> (Vec<Finding<'r>>, Vec<String>) {
    let recorded: BTreeMap<&str, &Change> = record
        .changes
        .iter()
        .map(|change| (change.id.as_str(), change))
        .collect();
    let mut findings = Vec::new();
    let mut stale = Vec::new();
    for (page, requirements) in by_page {
        let normalized = texts.get(page).map(|text| normalize(text));
        for requirement in requirements {
            let others = requirements
                .iter()
                .filter(|other| other.id != requirement.id)
                .map(|other| other.source.quote.as_str());
            let status = normalized.as_deref().map_or(Status::PageRemoved, |text| {
                compare::classify(text, &requirement.source.quote, others)
            });
            let row = recorded.get(requirement.id.as_str());
            let confirmed = row.is_some_and(|change| {
                normalized
                    .as_deref()
                    .is_some_and(|text| quote_present(text, &change.draft))
            });
            if row.is_some() && (!confirmed || status == Status::Present) {
                stale.push(requirement.id.to_string());
            }
            findings.push(Finding {
                requirement,
                page: page.clone(),
                status,
                recorded: confirmed,
            });
        }
    }
    (findings, stale)
}

/// The base revision's committed census, as tallies per page.
fn committed_tallies(sources: &Sources) -> BTreeMap<String, Tally> {
    sources
        .in_scope
        .keys()
        .map(|page| {
            let get = |numbers: &BTreeMap<String, u32>| numbers.get(page).copied().unwrap_or(0);
            let tally = Tally {
                must: get(&sources.must_census),
                should: get(&sources.should_census),
                may: get(&sources.may_census),
            };
            (page.clone(), tally)
        })
        .collect()
}

/// Appends the report to the job summary when running under Actions. A write
/// that fails is a warning: the report is already on stdout.
fn append_step_summary(markdown: &str) {
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
        return;
    };
    let written = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .and_then(|mut file| file.write_all(markdown.as_bytes()));
    if let Err(error) = written {
        eprintln!("xtask: spec-drift --draft — cannot append to $GITHUB_STEP_SUMMARY: {error}");
    }
}

#[cfg(test)]
mod tests;
