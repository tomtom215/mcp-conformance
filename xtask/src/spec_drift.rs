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

use mcp_conformance_core::requirement::{Registry, Requirement};
use serde::Deserialize;

/// The committed in-scope page set, relative to the workspace root.
const SOURCES: &str = "crates/mcp-conformance-core/registry/2025-11-25/sources.json";

/// Where the published spec text lives, per page file.
const RAW_BASE: &str = "https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2025-11-25";

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
    let registry = match Registry::builtin_2025_11_25() {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("xtask: spec-drift — embedded registry failed to load: {error}");
            return ExitCode::FAILURE;
        }
    };
    let sources = match load_sources() {
        Ok(sources) => sources,
        Err(message) => {
            eprintln!("xtask: spec-drift — {message}");
            return ExitCode::FAILURE;
        }
    };
    let by_page = requirements_by_page(&registry);
    if !sets_agree(&sources, &by_page) {
        return ExitCode::FAILURE;
    }

    let mut drifted = 0u32;
    for (page, requirements) in &by_page {
        let url = format!("{RAW_BASE}/{}", sources.in_scope[page]);
        let text = match fetch(&url) {
            Ok(text) => text,
            Err(message) => {
                eprintln!("xtask: spec-drift — cannot fetch {url}: {message}");
                return ExitCode::FAILURE;
            }
        };
        let normalized = normalize(&text);
        let mut page_drifted = 0u32;
        for requirement in requirements {
            if !quote_present(&normalized, &requirement.source.quote) {
                eprintln!(
                    "xtask: spec-drift — {}: quote no longer found on {page}:\n  {:?}",
                    requirement.id, requirement.source.quote
                );
                page_drifted += 1;
            }
        }
        eprintln!(
            "xtask: spec-drift — {page}: {} quote(s), {page_drifted} drifted (content {})",
            requirements.len(),
            fingerprint(&text)
        );
        drifted += page_drifted;
    }

    if drifted > 0 {
        eprintln!(
            "xtask: spec-drift — {drifted} quote(s) drifted from the published text. \
             Re-read each clause: if the requirement changed, update the entry (and \
             its checks or exclusion); if only the wording moved, refresh the quote."
        );
        return ExitCode::FAILURE;
    }
    eprintln!("xtask: spec-drift — every registry quote verified against the published text");
    ExitCode::SUCCESS
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

fn load_sources() -> Result<Sources, String> {
    let path = crate::workspace_root().join(SOURCES);
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

/// The normalization `SourceRef::quote` documents, applied to page text and
/// quotes alike: markdown bullet/number markers dropped, bold markers
/// dropped, typographic quotes straightened, whitespace runs collapsed —
/// and the quote convention's `"; "` list joins relaxed to single spaces on
/// both sides before matching.
fn normalize(text: &str) -> String {
    let mut joined = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        let without_marker = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| strip_numbered_marker(trimmed))
            .unwrap_or(trimmed);
        joined.push(' ');
        joined.push_str(without_marker);
    }
    let unstyled = strip_italics(&unwrap_links(&joined).replace("**", "").replace("\\_", "_"))
        .replace(['\u{201c}', '\u{201d}'], "\"")
        .replace('\u{2019}', "'");
    let mut collapsed = String::with_capacity(unstyled.len());
    let mut last_space = false;
    for ch in unstyled.chars() {
        if ch.is_whitespace() {
            if !last_space {
                collapsed.push(' ');
            }
            last_space = true;
        } else {
            collapsed.push(ch);
            last_space = false;
        }
    }
    collapsed.trim().to_owned()
}

/// Replaces every markdown link `[text](target)` with its text — quotes cite
/// the rendered words, and links may span source lines (handled because
/// unwrapping runs after line joining).
fn unwrap_links(text: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let Some(mid) = out.find("](") else {
            return out;
        };
        let Some(open) = out[..mid].rfind('[') else {
            return out;
        };
        let Some(close_rel) = out[mid + 2..].find(')') else {
            return out;
        };
        let close = mid + 2 + close_rel;
        let mut next = String::with_capacity(out.len());
        next.push_str(&out[..open]);
        next.push_str(&out[open + 1..mid]);
        next.push_str(&out[close + 1..]);
        out = next;
    }
}

/// Drops `_italic_` markers while keeping identifier underscores: an
/// underscore is a marker when a word character sits on exactly one side of
/// it — `_latest_` loses both, `list_changed` keeps its underscore (word
/// characters on both sides), and the rendered `(_)` keeps it (word
/// characters on neither side). Runs after escape unwrapping so MDX's
/// literal `\_` has already become a plain underscore.
fn strip_italics(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            let prev_word = index > 0 && chars[index - 1].is_alphanumeric();
            let next_word = chars.get(index + 1).is_some_and(|c| c.is_alphanumeric());
            if prev_word != next_word {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// `1. ` / `12. ` ordered-list markers.
fn strip_numbered_marker(line: &str) -> Option<&str> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    line.get(digits..)?.strip_prefix(". ")
}

/// Whether `quote` appears in the normalized page text: as one contiguous
/// run when it can, otherwise fragment-by-fragment on the `"; "` separators —
/// `SourceRef::quote`'s documented convention flattens lists and may keep
/// only the normative items, so the fragments are the verbatim units. The
/// fragment path cannot detect reordering, only rewording; the contiguous
/// path is tried first and covers every single-sentence quote.
fn quote_present(page_normalized: &str, quote: &str) -> bool {
    let relaxed_page = page_normalized.replace("; ", " ");
    let normalized_quote = normalize(quote);
    if relaxed_page.contains(&normalized_quote.replace("; ", " ")) {
        return true;
    }
    if normalized_quote
        .split("; ")
        .all(|fragment| !fragment.is_empty() && relaxed_page.contains(fragment))
    {
        return true;
    }
    // The convention's full shape: an introducing clause ending `:` whose
    // selected items follow. Verify the intro (with its colon) and each item
    // independently — LIFE-009 quotes the parent plus one of its bullets.
    if let Some((intro, items)) = normalized_quote.split_once(": ") {
        let intro_present = relaxed_page.contains(&format!("{intro}:"));
        let items_present = items
            .split("; ")
            .all(|fragment| !fragment.is_empty() && relaxed_page.contains(fragment));
        if intro_present && items_present {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
        let sources = load_sources().unwrap();
        let registry = Registry::builtin_2025_11_25().unwrap();
        let by_page = requirements_by_page(&registry);
        assert!(sets_agree(&sources, &by_page));
        assert_eq!(sources.in_scope.len(), 9, "the nine in-scope pages");
        assert!(!sources.out_of_scope.is_empty());
    }
}
