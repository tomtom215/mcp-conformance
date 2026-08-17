// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the tap records of an HTTP header map, and what it deliberately does
//! not.
//!
//! Split from [`super`] when that file crossed the 500-line cap, at the seam it
//! already had: the parent decides *which* exchanges become trace events, this
//! decides *how much* of each header map is kept. The allowlist is the privacy
//! boundary — a recording is an artifact people share — so it lives on its own
//! with the tests that pin it.

use std::collections::BTreeMap;

use axum::http::HeaderMap;

use super::{RECORDED_HEADER_PREFIXES, RECORDED_HEADERS};

/// The allowlisted subset of `headers`, lowercased.
///
/// Two passes because the allowlist has two shapes: the named headers are
/// looked up (so a name absent from the map costs nothing), and the prefixed
/// ones are found by walking the map (their full names are chosen by the
/// server under test, so there is nothing to look up).
pub(super) fn recorded_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    let named = RECORDED_HEADERS
        .iter()
        .filter_map(|name| header_value(headers, name).map(|value| ((*name).to_owned(), value)));
    let prefixed = headers
        .keys()
        .map(|name| name.as_str())
        .filter(|name| {
            RECORDED_HEADER_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
        .filter_map(|name| header_value(headers, name).map(|value| (name.to_owned(), value)));
    named.chain(prefixed).collect()
}

/// A header's value as UTF-8, combining repeated field lines.
///
/// HTTP permits a field to appear on multiple lines, semantically equivalent
/// to one comma-joined value (RFC 9110 §5.3). Recording only the first line
/// (`HeaderMap::get`) would misrepresent, e.g., an `Accept` split across two
/// lines — and the validator's `transport.client-accept-header` check reads
/// exactly this recorded value, so a lossy capture would manufacture a false
/// finding. We record the faithful combination instead. Returns `None` when
/// the field is absent or any line is non-UTF-8 (the latter never recorded
/// rather than recorded partially).
pub(super) fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut lines = headers.get_all(name).iter().peekable();
    lines.peek()?;
    let mut combined = String::new();
    for value in lines {
        let text = value.to_str().ok()?;
        if !combined.is_empty() {
            combined.push_str(", ");
        }
        combined.push_str(text);
    }
    Some(combined)
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn recorded_headers_is_an_allowlist() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        headers.insert("cookie", "id=1".parse().unwrap());
        headers.insert("host", "localhost:1234".parse().unwrap());
        headers.insert("mcp-session-id", "abc".parse().unwrap());
        let recorded = recorded_headers(&headers);
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded["host"], "localhost:1234");
        assert_eq!(recorded["mcp-session-id"], "abc");
        assert!(!recorded.contains_key("authorization"));
    }

    #[test]
    fn the_request_metadata_headers_a_check_reads_are_recorded() {
        // Each of these is the sole evidence for a `2026-07-28` clause, so a
        // recording that drops one turns a conforming exchange into a reported
        // violation. Pinned by name rather than by count: the point is which
        // headers, not how many.
        let mut headers = HeaderMap::new();
        headers.insert("mcp-method", "tools/call".parse().unwrap());
        headers.insert("mcp-name", "echo".parse().unwrap());
        headers.insert("mcp-param-region", "eu-west-1".parse().unwrap());
        headers.insert("x-accel-buffering", "no".parse().unwrap());
        let recorded = recorded_headers(&headers);
        assert_eq!(recorded["mcp-method"], "tools/call");
        assert_eq!(recorded["mcp-name"], "echo");
        assert_eq!(recorded["mcp-param-region"], "eu-west-1");
        assert_eq!(recorded["x-accel-buffering"], "no");
    }

    #[test]
    fn a_prefix_records_the_headers_it_names_and_nothing_beside_them() {
        // The prefix is the only allowlist shape that can cover header names a
        // *tool definition* chooses, so it must stay a prefix and not become a
        // substring: `authorization` does not begin with `mcp-param-`, and
        // neither does a header that merely contains it.
        let mut headers = HeaderMap::new();
        headers.insert("mcp-param-a", "1".parse().unwrap());
        headers.insert("x-mcp-param-b", "2".parse().unwrap());
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        let recorded = recorded_headers(&headers);
        assert_eq!(recorded.keys().collect::<Vec<_>>(), ["mcp-param-a"]);
    }

    #[test]
    fn header_value_combines_repeated_field_lines() {
        // An Accept split across two lines must record as the comma-joined
        // value, exactly as HTTP semantics combine them — otherwise the
        // validator's accept-header check would see only the first line and
        // manufacture a false finding on a perfectly legal request.
        let mut headers = HeaderMap::new();
        headers.append("accept", "application/json".parse().unwrap());
        headers.append("accept", "text/event-stream".parse().unwrap());
        assert_eq!(
            header_value(&headers, "accept").as_deref(),
            Some("application/json, text/event-stream")
        );
        // And through the allowlist path, so recording is faithful end to end.
        let recorded = recorded_headers(&headers);
        assert_eq!(recorded["accept"], "application/json, text/event-stream");
    }

    #[test]
    fn header_value_is_none_for_absent_fields() {
        let headers = HeaderMap::new();
        assert_eq!(header_value(&headers, "accept"), None);
    }
}
