// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Where a request goes upstream, and which responses are event streams.

use axum::http::uri::{PathAndQuery, Uri};
use axum::http::{HeaderMap, header};

/// `upstream` with the request's path appended to its own, and the request's
/// query appended to the upstream's (an upstream such as
/// `https://host/mcp?key=…` keeps its key).
pub(super) fn target(upstream: &Uri, path_and_query: Option<&PathAndQuery>) -> Uri {
    let base = upstream.path().trim_end_matches('/');
    let path = path_and_query.map_or("/", PathAndQuery::path);
    let request_query = path_and_query.and_then(PathAndQuery::query);
    let query = match (upstream.query(), request_query) {
        (Some(own), Some(request)) => Some(format!("{own}&{request}")),
        (Some(only), None) | (None, Some(only)) => Some(only.to_owned()),
        (None, None) => None,
    };
    let joined = query.map_or_else(
        || format!("{base}{path}"),
        |query| format!("{base}{path}?{query}"),
    );
    let mut parts = upstream.clone().into_parts();
    // `joined` is a path the client sent plus a prefix `Uri` already accepted, so it
    // is a valid path-and-query; fall back to the upstream's own if not.
    parts.path_and_query = joined.parse().ok().or(parts.path_and_query);
    Uri::from_parts(parts).unwrap_or_else(|_| upstream.clone())
}

pub(super) fn is_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
}
