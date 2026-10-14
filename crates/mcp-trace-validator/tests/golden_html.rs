// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Golden HTML reports: a representative set of corpus traces renders to a
//! byte-identical, committed page (`corpus/golden/html/`), and those pages are
//! what the screenshot suite (`visual/`) photographs.
//!
//! A subset, not the whole corpus. The JSON goldens already pin every trace's
//! verdict; what an HTML golden adds is the *rendering*, and that is exercised
//! by a handful of shapes — passing over each transport, failing a clause,
//! failing the wire schema, warning only, two revisions declared, two
//! revisions requested against a trace that declares one, and a run over
//! several traces with one left unjudged. Pinning all 205 traces would commit
//! 18.8 MB of pages (measured 2026-10-13; these nine are 0.8 MB) that differ mostly in
//! the accounting lists ADR-0013 moved out of the JSON goldens for exactly that
//! reason, and every stylesheet edit would rewrite all of them.
//!
//! The pages name the tool's version; it is replaced by `(version)` here so a
//! release does not rewrite every golden (and every screenshot) by one line.
//!
//! Regenerate with `BLESS=1 cargo test -p mcp-trace-validator --test
//! golden_html` (or `cargo xtask bless`) and review the diff like code.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mcp_trace_validator::html::{self, Trace};
use mcp_trace_validator::judge::{self, Judgment, Options};
use mcp_trace_validator::report::Unjudged;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden_dir() -> PathBuf {
    root().join("corpus/golden/html")
}

/// One trace, judged as the CLI judges it: at the revisions it declares, or
/// at `revisions` when given.
fn judge_trace(relative: &str, revisions: &[&str]) -> Judgment {
    let document = fs::read_to_string(root().join(relative)).unwrap();
    let options =
        Options::default().revisions(revisions.iter().map(|revision| revision.parse().unwrap()));
    judge::judge_with(&document, &options).unwrap()
}

/// The page with the tool's version replaced, as the golden pins it.
fn pinned(page: &str) -> String {
    let generator = format!("mcp-trace-validator {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        page.matches(&generator).count(),
        2,
        "the version appears in the generator meta and the footer, and nowhere else"
    );
    page.replace(&generator, "mcp-trace-validator (version)")
}

/// One trace's page, named by its path from the repository root.
fn single(relative: &'static str, revisions: &[&str]) -> String {
    let judgment = judge_trace(relative, revisions);
    html::render(
        &judgment.reports,
        Trace {
            name: Some(relative),
            events: &judgment.events,
        },
    )
}

/// A run over two traces and one that could not be read.
fn run_page() -> String {
    let minimal = judge_trace("corpus/good/stdio-minimal-init.jsonl", &[]);
    let reuse = judge_trace("corpus/violations/base-003-request-id-reuse.jsonl", &[]);
    html::render_run(
        &[
            (
                Trace {
                    name: Some("corpus/good/stdio-minimal-init.jsonl"),
                    events: &minimal.events,
                },
                &minimal.reports[..],
            ),
            (
                Trace {
                    name: Some("corpus/violations/base-003-request-id-reuse.jsonl"),
                    events: &reuse.events,
                },
                &reuse.reports[..],
            ),
        ],
        &[Unjudged::new(
            "missing.jsonl",
            "cannot read missing.jsonl: No such file or directory (os error 2)",
        )],
    )
}

/// The pinned pages: each name, and how to render it.
fn cases() -> Vec<(&'static str, String)> {
    vec![
        // Pass, over each transport and revision.
        (
            "http-session",
            single("corpus/good/http-session.jsonl", &[]),
        ),
        (
            "stdio-full-session",
            single("corpus/good/stdio-full-session.jsonl", &[]),
        ),
        (
            "draft-streamable-http-session",
            single("corpus/draft/good/streamable-http-session.jsonl", &[]),
        ),
        // Fail: a clause, and a clause with the wire schema.
        (
            "base-003-request-id-reuse",
            single("corpus/violations/base-003-request-id-reuse.jsonl", &[]),
        ),
        (
            "base-001-request-id-boolean",
            single("corpus/violations/base-001-request-id-boolean.jsonl", &[]),
        ),
        // Warnings only.
        (
            "tool-010-structured-without-text",
            single(
                "corpus/violations/tool-010-structured-without-text.jsonl",
                &[],
            ),
        ),
        // Two revisions: declared by the trace, and requested of a trace
        // declaring one (with the mismatch note).
        (
            "disc-002-dual-era-client-skips-probe",
            single(
                "corpus/draft/violations/disc-002-dual-era-client-skips-probe.jsonl",
                &[],
            ),
        ),
        (
            "stdio-full-session-two-revisions",
            single(
                "corpus/good/stdio-full-session.jsonl",
                &["2025-11-25", "2026-07-28"],
            ),
        ),
        // Several traces, one unjudged.
        ("run", run_page()),
    ]
}

fn blessing() -> bool {
    std::env::var("BLESS").is_ok_and(|value| value == "1")
}

#[test]
fn pages_match_their_goldens() {
    let mut mismatched = Vec::new();
    for (name, page) in cases() {
        let page = pinned(&page);
        let path = golden_dir().join(format!("{name}.html"));
        if blessing() {
            fs::create_dir_all(golden_dir()).unwrap();
            fs::write(&path, &page).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "cannot read {}: {error}\nhint: regenerate with `cargo xtask bless`",
                path.display()
            )
        });
        if page != expected {
            mismatched.push(path.display().to_string());
        }
    }
    assert!(
        mismatched.is_empty(),
        "pages diverge from their goldens: {mismatched:?}\nhint: if the change is intended, \
         run `cargo xtask bless`, review the diff, and update the screenshot baselines (visual/)"
    );
}

#[test]
fn every_html_golden_is_a_case() {
    if blessing() {
        return;
    }
    let cases: BTreeSet<String> = cases()
        .into_iter()
        .map(|(name, _)| name.to_owned())
        .collect();
    let committed: BTreeSet<String> = fs::read_dir(golden_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "html"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        committed, cases,
        "left: committed pages; right: the cases above"
    );
}

#[test]
fn the_pinned_pages_cover_every_verdict_a_trace_can_reach() {
    let pages: Vec<String> = cases().into_iter().map(|(_, page)| page).collect();
    for verdict in ["v-pass", "v-warn", "v-fail", "v-incomplete"] {
        assert!(
            pages
                .iter()
                .any(|page| page.contains(&format!("<span class=\"badge {verdict}\">"))),
            "no pinned page shows {verdict}"
        );
    }
    assert!(pages.iter().any(|page| page.contains("Wire schema: fail")));
    assert!(pages.iter().any(|page| page.contains("kind k-http")));
    assert!(
        pages
            .iter()
            .any(|page| page.contains("Judged by rules the session was not playing by"))
    );
}

/// Every corpus trace the CLI would judge, rendered: not pinned (see the module
/// note), but each page must be whole, fetch nothing, and link every finding to
/// an event that is on it.
#[test]
fn every_corpus_trace_renders_a_self_contained_page() {
    let mut rendered = 0;
    for dir in [
        "corpus/good",
        "corpus/violations",
        "corpus/draft/good",
        "corpus/draft/violations",
        "corpus/draft/captured",
    ] {
        let mut traces: Vec<PathBuf> = fs::read_dir(root().join(dir))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        traces.sort();
        for path in traces {
            let document = fs::read_to_string(&path).unwrap();
            // A trace the CLI refuses (never answered, judging nothing) has no page.
            let Ok(judgment) = judge::judge(&document) else {
                continue;
            };
            let page = html::render(
                &judgment.reports,
                Trace {
                    name: path.to_str(),
                    events: &judgment.events,
                },
            );
            let at = path.display();
            assert!(
                page.starts_with("<!DOCTYPE html>") && page.ends_with("</html>\n"),
                "{at}"
            );
            assert!(
                page.contains("http-equiv=\"Content-Security-Policy\""),
                "{at}"
            );
            for fetching in ["<script", "<link", "<img", "<iframe", " src=", " style=\""] {
                assert!(!page.contains(fetching), "{at}: {fetching}");
            }
            for target in page.split("<a class=\"seq\" href=\"#").skip(1) {
                let anchor = &target[..target.find('"').unwrap()];
                assert!(
                    page.contains(&format!(" id=\"{anchor}\"")),
                    "{at}: #{anchor} has no event"
                );
            }
            rendered += 1;
        }
    }
    assert!(rendered > 150, "only {rendered} corpus traces rendered");
}
