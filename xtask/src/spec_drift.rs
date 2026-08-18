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

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use mcp_conformance_core::requirement::{Registry, RegistrySet, Requirement};
use serde::Deserialize;

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
    #[allow(dead_code)]
    out_of_scope: BTreeMap<String, String>,
}

pub(crate) fn run() -> ExitCode {
    let set = match RegistrySet::builtin() {
        Ok(set) => set,
        Err(error) => {
            eprintln!("xtask: spec-drift — embedded registry set failed to load: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut drifted = 0u32;
    let mut checked = 0u32;
    let mut verified: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    for revision in REVISIONS {
        let Ok(parsed) = revision.parse() else {
            eprintln!("xtask: spec-drift — {revision} is not a protocol revision");
            return ExitCode::FAILURE;
        };
        // A revision the set does not describe has no embedded entries to verify —
        // the `draft-2026-07-28` feature being off is the ordinary case, not an error.
        let Some(registry) = set.registry(parsed) else {
            eprintln!("xtask: spec-drift — {revision}: not described by this build, skipped");
            skipped.push(revision);
            continue;
        };
        verified.push(revision);
        match verify_revision(revision, &registry) {
            Ok(count) => {
                checked += count.0;
                drifted += count.1;
            }
            Err(()) => return ExitCode::FAILURE,
        }
    }

    if drifted > 0 {
        eprintln!(
            "xtask: spec-drift — {drifted} quote(s) drifted from the published text. \
             Re-read each clause: if the requirement changed, update the entry (and \
             its checks or exclusion); if only the wording moved, refresh the quote."
        );
        return ExitCode::FAILURE;
    }
    // The count is of revisions *verified*, not of revisions looped over. It
    // used to be `REVISIONS.len()`, so a build without the draft feature
    // announced "140 quote(s) across 2 revision(s)" having read one of them —
    // a gate overstating its own coverage, which is the failure this gate
    // exists to catch in other people's documents.
    eprintln!(
        "xtask: spec-drift — {checked} quote(s) across {} revision(s) verified against \
         the published text{}",
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

/// Verifies one revision's quotes against its own published pages.
///
/// Returns `(quotes checked, quotes drifted)`, or `Err(())` when the revision could not
/// be verified at all — an unreadable sources file, a page/registry disagreement, or a
/// failed fetch. An unverified page is not a verified page.
fn verify_revision(revision: &str, registry: &Registry) -> Result<(u32, u32), ()> {
    let sources = load_sources(revision).map_err(|message| {
        eprintln!("xtask: spec-drift — {revision}: {message}");
    })?;
    let by_page = requirements_by_page(registry);
    if !sets_agree(&sources, &by_page) {
        return Err(());
    }

    let mut checked = 0u32;
    let mut drifted = 0u32;
    for (page, requirements) in &by_page {
        let url = format!("{}/{}", raw_base(revision), sources.in_scope[page]);
        let text = fetch(&url).map_err(|message| {
            eprintln!("xtask: spec-drift — cannot fetch {url}: {message}");
        })?;
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
    }
    Ok((checked, drifted))
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

mod quote;

use quote::{normalize, quote_present};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    // Private to the quote module now; the test stays here with its siblings.
    use super::quote::strip_numbered_marker;

    #[test]
    fn normalization_flattens_lists_the_way_quotes_are_written() {
        // The SourceRef::quote convention: list items joined with "; " after
        // the introducing ":". The page text has them as markdown bullets.
        let page = "The client MUST send a request containing:\n\n- Protocol version supported\n- Client capabilities\n- Client implementation information\n";
        let quote = "The client MUST send a request containing: Protocol version supported; Client capabilities; Client implementation information";
        assert!(quote_present(&normalize(page), quote));
    }

    #[test]
    fn drifted_text_is_not_matched() {
        let page = "The server SHOULD respond promptly.";
        let quote = "The server MUST respond promptly.";
        assert!(!quote_present(&normalize(page), quote));
    }

    #[test]
    fn bold_and_typographic_punctuation_are_normalized() {
        let page = "Servers **MUST** validate the \u{201c}Origin\u{201d} header.";
        let quote = "Servers MUST validate the \"Origin\" header.";
        assert!(quote_present(&normalize(page), quote));
    }

    #[test]
    fn numbered_markers_strip_like_bullets() {
        assert_eq!(strip_numbered_marker("1. First"), Some("First"));
        assert_eq!(strip_numbered_marker("12. Twelfth"), Some("Twelfth"));
        assert_eq!(strip_numbered_marker("1.5a not a marker"), None);
        assert_eq!(strip_numbered_marker("no digits"), None);
    }

    #[test]
    fn links_unwrap_to_their_text_even_across_lines() {
        let page = "Custom URI schemes **MUST** be in accordance with [RFC3986](https://datatracker.ietf.org/doc/html/rfc3986),\ntaking the above guidance in to account.";
        let quote = "Custom URI schemes MUST be in accordance with RFC3986, taking the above guidance in to account.";
        assert!(quote_present(&normalize(page), quote));
        let cross_line = "declare it during\n[initialization](/specification/x#initialization):";
        assert!(quote_present(
            &normalize(cross_line),
            "declare it during initialization:"
        ));
    }

    #[test]
    fn mdx_escaped_underscores_match_their_rendered_form() {
        let page = "underscore (\\_), hyphen (-), and dot (.)";
        assert!(quote_present(
            &normalize(page),
            "underscore (_), hyphen (-), and dot (.)"
        ));
    }

    #[test]
    fn selected_list_fragments_verify_individually() {
        // The extraction convention may join a parent with one selected
        // sub-item, skipping non-normative siblings: each fragment must
        // still be verbatim on the page.
        let page = "- `inputSchema`: JSON Schema defining expected parameters\n  - Follows the guidelines\n  - **MUST** be a valid JSON Schema object (not `null`)\n";
        let quote = "`inputSchema`: JSON Schema defining expected parameters; MUST be a valid JSON Schema object (not `null`)";
        assert!(quote_present(&normalize(page), quote));
        // A reworded fragment still fails.
        let drifted = "`inputSchema`: JSON Schema defining expected parameters; MUST be a valid JSON Schema object or null";
        assert!(!quote_present(&normalize(page), drifted));
    }

    #[test]
    fn italic_markers_strip_but_identifier_underscores_survive() {
        let page = "This **SHOULD** be the _latest_ version; see `notifications/tools/list_changed` and the rendered (\\_) escape.";
        let normalized = normalize(page);
        assert!(normalized.contains("the latest version"), "{normalized}");
        assert!(normalized.contains("list_changed"), "{normalized}");
        assert!(normalized.contains("(_)"), "{normalized}");
        assert!(quote_present(
            &normalize(
                "the server **MUST** either return `Content-Type: text/event-stream`, to initiate an SSE stream, or `Content-Type: application/json`, to return one JSON object, when the input is a JSON-RPC _request_"
            ),
            "the server MUST either return `Content-Type: text/event-stream`, to initiate an SSE stream, or `Content-Type: application/json`, to return one JSON object, when the input is a JSON-RPC request"
        ));
    }

    #[test]
    fn intro_colon_quotes_verify_parent_and_selected_item() {
        let page = "Both parties **MUST**:\n\n- Respect the negotiated protocol version\n- Only use capabilities that were successfully negotiated\n";
        let quote = "Both parties MUST: Only use capabilities that were successfully negotiated";
        assert!(quote_present(&normalize(page), quote));
        // The intro alone is not enough: a deleted item must still drift.
        let drifted = "Both parties MUST: Only use capabilities that were never negotiated";
        assert!(!quote_present(&normalize(page), drifted));
    }

    #[test]
    fn the_committed_sources_file_parses_and_matches_the_registry() {
        // The offline halves of the gate, pinned in `cargo test`: the file
        // parses strictly and the two page sets agree. The network half
        // (quote verification) runs in the scheduled job.
        let sources = load_sources("2025-11-25").unwrap();
        let registry = Registry::builtin_2025_11_25().unwrap();
        let by_page = requirements_by_page(&registry);
        assert!(sets_agree(&sources, &by_page));
        assert_eq!(sources.in_scope.len(), 9, "the nine in-scope pages");
        assert!(!sources.out_of_scope.is_empty());
    }

    #[test]
    fn pages_and_sources_resolve_under_their_own_revision() {
        // The point of deriving both from a revision rather than fixing them:
        // with `2026-07-28` published beside `2025-11-25`, a quote checked
        // against the wrong revision's page would drift silently in whichever
        // direction the two texts happen to agree.
        assert!(raw_base("2026-07-28").ends_with("/docs/specification/2026-07-28"));
        assert!(
            sources_path("2026-07-28")
                .starts_with("crates/mcp-conformance-core/registry/2026-07-28/")
        );
        assert_ne!(raw_base("2025-11-25"), raw_base("2026-07-28"));
    }
}
