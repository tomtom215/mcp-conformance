// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Where a request goes upstream, where a redirect sends the client back to, and
//! which responses are event streams.
//!
//! The mapping: the proxy's `/` is the upstream URL itself, and any other path is
//! appended to the upstream's path. So with `--upstream http://h/mcp` a client
//! pointed at the proxy's root reaches exactly `/mcp` — appending `/` made it
//! `/mcp/`, which servers answer with a redirect to `/mcp` — and the proxy's
//! `/other` reaches `/mcp/other`.

use axum::http::uri::{PathAndQuery, Uri};
use axum::http::{HeaderMap, header};

/// `upstream` with the request's path appended to its own (the upstream itself
/// for the root path), and the request's query appended to the upstream's (an
/// upstream such as `https://host/mcp?key=…` keeps its key).
pub(super) fn target(upstream: &Uri, path_and_query: Option<&PathAndQuery>) -> Uri {
    let base = upstream.path().trim_end_matches('/');
    let path = path_and_query.map_or("/", PathAndQuery::path);
    let request_query = path_and_query.and_then(PathAndQuery::query);
    let query = match (upstream.query(), request_query) {
        (Some(own), Some(request)) => Some(format!("{own}&{request}")),
        (Some(only), None) | (None, Some(only)) => Some(only.to_owned()),
        (None, None) => None,
    };
    let path = if path == "/" {
        upstream.path().to_owned()
    } else {
        format!("{base}{path}")
    };
    let joined = query.map_or_else(|| path.clone(), |query| format!("{path}?{query}"));
    let mut parts = upstream.clone().into_parts();
    // `joined` is a path the client sent plus a prefix `Uri` already accepted, so it
    // is a valid path-and-query; fall back to the upstream's own if not.
    parts.path_and_query = joined.parse().ok().or(parts.path_and_query);
    Uri::from_parts(parts).unwrap_or_else(|_| upstream.clone())
}

/// A `Location` the upstream sent, pointed back through the proxy — `None` to
/// relay it as sent.
///
/// A redirect into the upstream would otherwise take the client straight to the
/// server, past the proxy and out of the trace. Rewritten: a URL on the
/// upstream's own scheme and authority, or a path (`/mcp`) on the same server,
/// whose path the proxy can reach — the inverse of [`target`]. An absolute one
/// becomes `http://` plus `proxy_host`, the proxy's own address the client
/// reached (never the client's `Host` header, which names whatever the client
/// likes). Anything else — another server, a relative reference, a path
/// outside the upstream's, a query that does not carry the upstream's own — is
/// left alone.
pub(super) fn location(upstream: &Uri, value: &str, proxy_host: Option<&str>) -> Option<String> {
    let sent: Uri = value.parse().ok()?;
    let host = match (sent.scheme(), sent.authority()) {
        (Some(scheme), Some(authority)) => {
            let same = Some(scheme) == upstream.scheme()
                && upstream
                    .authority()
                    .is_some_and(|own| own.as_str().eq_ignore_ascii_case(authority.as_str()));
            if !same {
                return None;
            }
            Some(proxy_host?)
        }
        (None, None) if value.starts_with('/') && !value.starts_with("//") => None,
        _ => return None,
    };
    let path = proxy_path(upstream.path(), sent.path())?;
    let query = match (upstream.query(), sent.query()) {
        (None, query) => query,
        (Some(own), Some(query)) if query == own => None,
        (Some(own), Some(query)) => Some(query.strip_prefix(own)?.strip_prefix('&')?),
        (Some(_), None) => return None,
    };
    let path_and_query = query.map_or_else(|| path.to_owned(), |query| format!("{path}?{query}"));
    Some(host.map_or_else(
        || path_and_query.clone(),
        |host| format!("http://{host}{path_and_query}"),
    ))
}

/// The proxy path [`target`] maps to the upstream path `path`, if any.
fn proxy_path<'a>(upstream: &str, path: &'a str) -> Option<&'a str> {
    if path == upstream {
        return Some("/");
    }
    let rest = path.strip_prefix(upstream.trim_end_matches('/'))?;
    (rest.starts_with('/') && rest != "/").then_some(rest)
}

/// Whether `path` has a `.` or `..` segment — spelled out, or with its dots
/// percent-encoded (`%2e`), or between encoded separators (`%2f`, and `\` or
/// `%5c`, which some servers also split on). Appended to the upstream's path,
/// such a segment would leave it once the server resolves it: `/../secret`
/// against `http://h/mcp` is `/mcp/../secret`, which is `/secret`.
pub(super) fn has_dot_segment(path: &str) -> bool {
    let mut decoded = path.to_owned();
    for (encoded, plain) in [("%2e", "."), ("%2f", "/"), ("%5c", "\\")] {
        for spelling in [encoded.to_owned(), encoded.to_ascii_uppercase()] {
            decoded = decoded.replace(&spelling, plain);
        }
    }
    decoded
        .split(['/', '\\'])
        .any(|segment| segment == "." || segment == "..")
}

pub(super) fn is_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
}
