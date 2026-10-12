// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the versioned documentation site.

use super::*;

#[test]
fn only_plain_release_tags_are_releases() {
    assert_eq!(parse_release_tag("v0.6.0"), Some((0, 6, 0)));
    assert_eq!(parse_release_tag("v10.20.30"), Some((10, 20, 30)));
    for not_a_release in [
        "0.6.0",
        "v0.6",
        "v0.6.0.1",
        "v0.7.0-rc.1",
        "v0.6.0+build",
        "v01.2.3",
        "v1..2",
        "vx.y.z",
        "",
    ] {
        assert_eq!(parse_release_tag(not_a_release), None, "{not_a_release}");
    }
}

#[test]
fn releases_are_newest_first_by_version_not_by_text() {
    let tags = "v0.10.0\nv0.9.1\nv0.2.0\nv0.7.0-rc.1\nnot-a-tag\nv0.9.0\n";
    let releases = released_books(tags, |_| true);
    let order: Vec<&str> = releases
        .iter()
        .map(|release| release.tag.as_str())
        .collect();
    assert_eq!(order, ["v0.10.0", "v0.9.1", "v0.9.0", "v0.2.0"]);
}

#[test]
fn a_tag_without_a_book_is_left_out() {
    let releases = released_books("v0.1.0\nv0.3.0\n", |tag| tag != "v0.1.0");
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].tag, "v0.3.0");
}

#[test]
fn the_newest_release_is_offered_at_the_root_and_dev_last() {
    let releases = released_books("v0.5.1\nv0.6.0\n", |_| true);
    let document: serde_json::Value = serde_json::from_str(&versions_json(&releases)).unwrap();
    assert_eq!(document["latest"], "v0.6.0");
    assert_eq!(
        document["versions"],
        serde_json::json!([
            { "id": "v0.6.0", "label": "0.6.0 (latest)", "path": "" },
            { "id": "v0.5.1", "label": "0.5.1", "path": "v0.5.1/" },
            { "id": "dev", "label": "main (unreleased)", "path": "dev/" },
        ])
    );
}

#[test]
fn with_no_release_dev_is_the_root() {
    let document: serde_json::Value = serde_json::from_str(&versions_json(&[])).unwrap();
    assert_eq!(document["latest"], serde_json::Value::Null);
    assert_eq!(
        document["versions"],
        serde_json::json!([{ "id": "dev", "label": "main (unreleased)", "path": "" }])
    );
}

const PAGE: &str = "<html><head><title>t</title></head><body></body></html>";

#[test]
fn a_root_page_gets_the_switcher_and_stays_indexable() {
    let annotated = annotate(PAGE, Path::new("ci.html"), &["v0.6.0", "dev"]).unwrap();
    assert_eq!(
        annotated,
        "<html><head><title>t</title><script src=\"versions.js\" defer></script>\n\
         </head><body></body></html>"
    );
}

#[test]
fn a_versioned_page_is_noindex_and_finds_the_switcher_at_the_site_root() {
    let annotated = annotate(PAGE, Path::new("v0.5.1/ci.html"), &["v0.5.1", "dev"]).unwrap();
    assert!(
        annotated.contains("<meta name=\"robots\" content=\"noindex\">"),
        "{annotated}"
    );
    assert!(
        annotated.contains("<script src=\"../versions.js\" defer>"),
        "{annotated}"
    );
    let nested = annotate(PAGE, Path::new("dev/a/b.html"), &["dev"]).unwrap();
    assert!(
        nested.contains("<script src=\"../../versions.js\" defer>"),
        "{nested}"
    );
    assert!(nested.contains("noindex"), "{nested}");
}

#[test]
fn a_page_mdbook_already_marks_gets_no_second_robots_tag() {
    let marked = "<head><meta name=\"robots\" content=\"noindex\"></head>";
    let annotated = annotate(marked, Path::new("dev/print.html"), &["dev"]).unwrap();
    assert_eq!(
        annotated.matches("name=\"robots\"").count(),
        1,
        "{annotated}"
    );
}

#[test]
fn a_root_directory_that_is_not_a_version_stays_indexable() {
    // mdBook writes no such directory today; a future one (say `assets/`)
    // must not be mistaken for an old version and hidden from search.
    let annotated = annotate(PAGE, Path::new("assets/x.html"), &["v0.6.0", "dev"]).unwrap();
    assert!(!annotated.contains("noindex"), "{annotated}");
    assert!(
        annotated.contains("<script src=\"../versions.js\" defer>"),
        "{annotated}"
    );
}

#[test]
fn the_head_is_annotated_once_before_its_first_close() {
    let two = "<head></head><template><head></head></template>";
    let annotated = annotate(two, Path::new("x.html"), &[]).unwrap();
    assert_eq!(annotated.matches("versions.js").count(), 1);
    assert!(annotated.starts_with("<head><script"), "{annotated}");
    assert_eq!(annotate("<p>no head</p>", Path::new("x.html"), &[]), None);
}

#[test]
fn a_release_book_links_to_its_own_tag() {
    let html = r#"<a href="https://github.com/tomtom215/mcp-conformance/tree/main/interop">interop</a> <a href="https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/README.md">plan</a> <a href="https://github.com/tomtom215/mcp-conformance/releases">releases</a> <a href="https://github.com/other/repo/blob/main/x">other</a>"#;
    let pinned = pin_links(html, "v0.6.0");
    assert!(
        pinned.contains("mcp-conformance/tree/v0.6.0/interop"),
        "{pinned}"
    );
    assert!(
        pinned.contains("mcp-conformance/blob/v0.6.0/docs/plan/README.md"),
        "{pinned}"
    );
    // Not a file on `main`, or not this repository: left alone.
    assert!(pinned.contains("mcp-conformance/releases"), "{pinned}");
    assert!(pinned.contains("other/repo/blob/main/x"), "{pinned}");
    assert!(!pinned.contains("mcp-conformance/tree/main/"), "{pinned}");
}
