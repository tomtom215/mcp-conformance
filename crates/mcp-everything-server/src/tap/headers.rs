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

use super::RECORDED_HEADERS;

/// The allowlisted subset of `headers`, lowercased.
pub(super) fn recorded_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    RECORDED_HEADERS
        .iter()
        .filter_map(|name| header_value(headers, name).map(|value| ((*name).to_owned(), value)))
        .collect()
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
