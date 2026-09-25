// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which headers a recording keeps, and which a proxy must not forward.
//!
//! Recording uses the allowlist every capture shares
//! ([`RECORDED_HEADERS`], [`RECORDED_HEADER_PREFIXES`]): credentials are never
//! written because they are never on the list. Forwarding is the opposite shape — a
//! blocklist of hop-by-hop fields (RFC 9110 §7.6.1), which describe one connection
//! and are the proxy's to set, not the endpoints'.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, HeaderName, header};
use mcp_conformance_core::trace::{RECORDED_HEADER_PREFIXES, RECORDED_HEADERS};

/// The allowlisted subset of `headers`, with names lowercased and repeated field
/// lines joined by `, ` (RFC 9110 §5.3). A field with any non-UTF-8 line is omitted
/// rather than recorded partially.
#[must_use]
pub fn recorded(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .keys()
        .map(HeaderName::as_str)
        .filter(|name| {
            RECORDED_HEADERS.contains(name)
                || RECORDED_HEADER_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
        })
        .filter_map(|name| joined(headers, name).map(|value| (name.to_owned(), value)))
        .collect()
}

fn joined(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut combined = String::new();
    for (index, value) in headers.get_all(name).iter().enumerate() {
        if index > 0 {
            combined.push_str(", ");
        }
        combined.push_str(value.to_str().ok()?);
    }
    Some(combined)
}

/// Fields that belong to a single connection (RFC 9110 §7.6.1).
const HOP_BY_HOP: [HeaderName; 8] = [
    header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
];

/// `headers` minus hop-by-hop fields, minus any field the `Connection` header names,
/// minus `extra`.
#[must_use]
pub fn forwardable(headers: &HeaderMap, extra: &[HeaderName]) -> HeaderMap {
    let named_by_connection: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect();
    let mut out = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        if HOP_BY_HOP.contains(name)
            || extra.contains(name)
            || named_by_connection
                .iter()
                .any(|token| token == name.as_str())
        {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn recording_keeps_the_allowlist_and_never_credentials() {
        let headers = map(&[
            ("Authorization", "Bearer secret"),
            ("Cookie", "session=secret"),
            ("Mcp-Session-Id", "abc"),
            ("Accept", "application/json"),
            ("Accept", "text/event-stream"),
            ("Mcp-Param-Region", "eu"),
            ("X-Unrelated", "1"),
        ]);
        let recorded = recorded(&headers);
        assert_eq!(
            recorded.keys().map(String::as_str).collect::<Vec<_>>(),
            ["accept", "mcp-param-region", "mcp-session-id"]
        );
        assert_eq!(recorded["accept"], "application/json, text/event-stream");
        assert!(!format!("{recorded:?}").contains("secret"));
    }

    #[test]
    fn forwarding_drops_hop_by_hop_fields_and_those_connection_names() {
        let headers = map(&[
            ("Connection", "keep-alive, X-Hop"),
            ("Keep-Alive", "timeout=5"),
            ("X-Hop", "1"),
            ("Transfer-Encoding", "chunked"),
            ("Host", "proxy:8080"),
            ("Authorization", "Bearer kept"),
            ("Mcp-Protocol-Version", "2026-07-28"),
        ]);
        let forwarded = forwardable(&headers, &[header::HOST]);
        let names: Vec<&str> = forwarded.keys().map(HeaderName::as_str).collect();
        // Credentials are forwarded — the proxy must be transparent — even though
        // they are never recorded.
        assert_eq!(names, ["authorization", "mcp-protocol-version"]);
    }
}
