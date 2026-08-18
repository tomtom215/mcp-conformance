// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The book's worked examples, executed.
//!
//! The sibling of `readme_examples.rs`, for the same reason: a documentation
//! example that drifts from the tool's real output is a small lie with a long
//! shelf life. The book is also the one place a reader meets the multi-revision
//! report, and its whole point is that the numbers are honest — so the page
//! quoting them is held to the tool, not to a reviewer's memory.
//!
//! Feature-gated because the example judges `2026-07-28`, which is not a
//! default feature; without it there is no second revision to judge against.

#![cfg(feature = "draft-2026-07-28")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_conformance_core::requirement::RegistrySet;
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_trace_validator::multi;
use mcp_trace_validator::reader::{Limits, parse_trace};

fn chapter() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../book/src/revisions.md");
    std::fs::read_to_string(path).expect("the revisions chapter exists")
}

/// The content of every fenced block whose info string is `lang`.
fn fenced_blocks(text: &str, lang: &str) -> Vec<String> {
    let open = format!("```{lang}\n");
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = text[at..].find(&open) {
        let start = at + offset + open.len();
        let end = text[start..].find("```").expect("closing fence") + start;
        found.push(text[start..end].to_owned());
        at = end;
    }
    found
}

#[test]
fn the_chapters_multi_revision_example_is_what_the_validator_prints() {
    let chapter = chapter();
    let trace = fenced_blocks(&chapter, "jsonl")
        .pop()
        .expect("the chapter shows one trace");

    let set = RegistrySet::builtin().unwrap();
    let revisions: Vec<ProtocolRevision> = ["2025-11-25", "2026-07-28"]
        .iter()
        .map(|revision| revision.parse().unwrap())
        .collect();
    let events = parse_trace(&trace, &Limits::default()).expect("the chapter's trace parses");
    let rendered = multi::validate_revisions(&set, &revisions, &events)
        .unwrap()
        .render_human();

    // Every `text` block except the invocation is quoted output. Checked line by
    // line so a stale row is named, rather than the whole block failing at once.
    for block in fenced_blocks(&chapter, "text") {
        if block.starts_with("$ ") {
            continue;
        }
        for line in block.lines().filter(|line| !line.trim().is_empty()) {
            assert!(
                rendered.contains(line),
                "the chapter quotes a line the validator does not produce:\n  {line}\nactual:\n{rendered}"
            );
        }
    }
}

#[test]
fn the_four_outcomes_the_chapter_tabulates_all_occur_in_it() {
    // The page claims the side-by-side report is where `absent`, `excluded`,
    // `not-applicable` and `not-observed` appear at once and mean different
    // things. If the example stopped producing all four, the table beside it
    // would be teaching from a case the reader cannot see.
    let chapter = chapter();
    let trace = fenced_blocks(&chapter, "jsonl").pop().unwrap();
    let set = RegistrySet::builtin().unwrap();
    let revisions: Vec<ProtocolRevision> = ["2025-11-25", "2026-07-28"]
        .iter()
        .map(|revision| revision.parse().unwrap())
        .collect();
    let events = parse_trace(&trace, &Limits::default()).unwrap();
    let rendered = multi::validate_revisions(&set, &revisions, &events)
        .unwrap()
        .render_human();
    for token in ["=absent", "=excluded", "=not-applicable", "=not-observed"] {
        assert!(rendered.contains(token), "{token} never appears");
    }
}
