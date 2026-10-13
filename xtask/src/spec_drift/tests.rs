// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the spec-drift gate: quote matching and the offline halves.
//!
//! Split from the parent module for the 500-line cap, unchanged.

#![allow(clippy::unwrap_used)]

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
fn table_row_quotes_verify_as_the_row_reads() {
    // The `SourceRef::quote` form for a clause that only reads as one with its
    // row: first cell through the clause's cell, joined with ` | `. Column
    // padding collapses like any other whitespace.
    let page = "| Scenario | Client Behavior | Server Behavior |\n\
                | --- | --- | --- |\n\
                | Parameter value is `null`   | Client MUST omit the header    | Server MUST NOT expect the header |\n";
    let normalized = normalize(page);
    assert!(quote_present(
        &normalized,
        "Parameter value is `null` | Client MUST omit the header"
    ));
    assert!(quote_present(
        &normalized,
        "Parameter value is `null` | Client MUST omit the header | Server MUST NOT expect the header"
    ));
    // A cell moved to another row does not verify.
    assert!(!quote_present(
        &normalized,
        "Parameter value is `null` | Server MUST NOT expect the header"
    ));
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
    assert_eq!(sources.in_scope.len(), 12, "the twelve in-scope pages");
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
        sources_path("2026-07-28").starts_with("crates/mcp-conformance-core/registry/2026-07-28/")
    );
    assert_ne!(raw_base("2025-11-25"), raw_base("2026-07-28"));
}
