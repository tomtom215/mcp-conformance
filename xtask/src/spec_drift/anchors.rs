// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Every registry anchor resolves on the published page.
//!
//! A report row links its clause as `SourceRef::url` builds it, so an anchor the
//! site does not publish sends the reader to the top of a long page — silently,
//! since the page itself still loads. The registry's anchors were written as
//! GitHub-style slugs of the spec source's headings, and the site slugs
//! differently wherever a heading has punctuation (`_meta`, `$ref-resolution`,
//! `security-&-endpoint`, `https//`) and gives no anchor at all below `####`.
//! Deriving the site's rule from the source would be a guess fitted to the
//! headings seen so far; reading the anchors off the published HTML is not.

use std::collections::BTreeSet;

use mcp_conformance_core::requirement::{Registry, Requirement};

/// How many of one page's requirements cite an anchor the published page lacks,
/// each reported by ID. A section without an anchor cites the whole page, which
/// always resolves.
pub(super) fn broken(registry: &Registry, requirements: &[&Requirement]) -> Result<u32, ()> {
    let Some(first) = requirements.first() else {
        return Ok(0);
    };
    let url = first.source.url(registry.revision());
    let page = url.split_once('#').map_or(url.as_str(), |(page, _)| page);
    let html = super::fetch(page).map_err(|message| {
        eprintln!("xtask: spec-drift — cannot fetch {page}: {message}");
    })?;
    let offered = heading_ids(&html);
    let mut broken = 0u32;
    for requirement in requirements {
        let Some((_, anchor)) = requirement.source.section.split_once('#') else {
            continue;
        };
        if !offered.contains(anchor) {
            eprintln!(
                "xtask: spec-drift — {}: {page} publishes no anchor #{anchor}",
                requirement.id
            );
            broken += 1;
        }
    }
    Ok(broken)
}

/// The anchors a published page offers: the `id` of every heading element.
///
/// Headings only, because the site's chrome carries ids too (`navbar`,
/// `sidebar`) and a section anchor that happened to equal one would pass
/// without pointing at any section.
fn heading_ids(html: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut rest = html;
    while let Some(start) = rest.find("<h") {
        rest = &rest[start + 2..];
        let is_heading = rest
            .as_bytes()
            .first()
            .is_some_and(|level| (b'1'..=b'6').contains(level));
        let Some(end) = rest.find('>') else { break };
        if is_heading && let Some(id) = attribute(&rest[..end], "id") {
            ids.insert(unescape(id));
        }
        rest = &rest[end..];
    }
    ids
}

/// The double-quoted value of `name` within one tag's attribute text.
fn attribute<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// The five entities HTML attribute serialization produces.
fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_ids_are_read_and_unescaped() {
        let html = r#"<div id="navbar"></div><h2 class="x" id="security-&amp;-endpoint">S</h2>
<h3 id="_meta"><span>m</span></h3><h4 data-a="1" id="$ref-resolution">r</h4>
<header id="header"></header><hr id="rule"/>"#;
        let ids: Vec<String> = heading_ids(html).into_iter().collect();
        assert_eq!(ids, ["$ref-resolution", "_meta", "security-&-endpoint"]);
    }

    #[test]
    fn a_heading_without_an_id_contributes_nothing() {
        assert!(heading_ids("<h5>ResultType</h5><h2 >x</h2>").is_empty());
    }

    #[test]
    fn an_unterminated_tag_ends_the_scan_without_panicking() {
        assert!(heading_ids(r#"<h2 id="a""#).is_empty());
        assert!(heading_ids("<h").is_empty());
    }
}
