// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Where a server that answered `401` keeps its Protected Resource Metadata
//! (RFC 9728), as a client works it out: the URL the challenge names, or
//! failing that the well-known locations for the endpoint.
//!
//! Only a location on the server's own origin and under `/.well-known/` is
//! fetched: the fetch goes through the recorder, which reaches that origin
//! only and keeps a path only there.

use axum::http::Uri;
use mcp_conformance_core::trace::records_path;

/// The well-known location's first segment (RFC 9728 §3).
const WELL_KNOWN: &str = "/.well-known/oauth-protected-resource";

/// What to fetch for a server's metadata.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// The path of the URL the challenge names.
    Named(String),
    /// The challenge names a URL this run cannot fetch, and why.
    Elsewhere(&'static str),
    /// No URL was named: the well-known locations, the endpoint's own first.
    WellKnown(Vec<String>),
}

/// The `resource_metadata` parameter of the challenge `header`, when it has
/// one: the quoted string's content, or the bare token.
pub(crate) fn named(header: &str) -> Option<&str> {
    const NAME: &str = "resource_metadata";
    let lower = header.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(NAME) {
        let start = from + found;
        from = start + NAME.len();
        // A parameter name, not the tail of another one.
        let whole = header[..start]
            .chars()
            .next_back()
            .is_none_or(|before| before == ' ' || before == ',' || before == '\t');
        let Some(value) = header[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        if !whole {
            continue;
        }
        let value = value.trim_start();
        let end = |text: &str, ends: &[char]| text.find(ends).unwrap_or(text.len());
        return Some(value.strip_prefix('"').map_or_else(
            || &value[..end(value, &[',', ' ', '\t'])],
            |quoted| &quoted[..end(quoted, &['"'])],
        ));
    }
    None
}

/// What to fetch, given the `challenge` the server sent (if any), its origin
/// `upstream` and the `endpoint` path (and query) the session posts to.
pub(crate) fn plan(challenge: Option<&str>, upstream: &Uri, endpoint: &str) -> Plan {
    let Some(url) = challenge.and_then(named) else {
        let path = endpoint.split('?').next().unwrap_or_default();
        let mut paths = Vec::new();
        if path.len() > 1 {
            paths.push(format!("{WELL_KNOWN}{}", path.trim_end_matches('/')));
        }
        paths.push(WELL_KNOWN.to_owned());
        paths.dedup();
        return Plan::WellKnown(paths);
    };
    let Ok(url) = url.parse::<Uri>() else {
        return Plan::Elsewhere("it is not a URL");
    };
    let same_origin = url.scheme() == upstream.scheme()
        && match (url.authority(), upstream.authority()) {
            (Some(named), Some(own)) => named.as_str().eq_ignore_ascii_case(own.as_str()),
            _ => false,
        };
    if !same_origin {
        return Plan::Elsewhere("it is on another origin than the URL given");
    }
    if !records_path(url.path()) {
        return Plan::Elsewhere("it is not under /.well-known/");
    }
    Plan::Named(url.path().to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_parameter_is_read_quoted_or_bare_and_only_by_its_whole_name() {
        for (header, value) in [
            (
                r#"Bearer resource_metadata="https://h/x""#,
                Some("https://h/x"),
            ),
            (r#"Bearer scope="a", Resource_Metadata = "u""#, Some("u")),
            (
                "Bearer resource_metadata=https://h/x, scope=a",
                Some("https://h/x"),
            ),
            ("Bearer resource_metadata=u", Some("u")),
            (
                r#"Bearer x_resource_metadata="no", resource_metadata="yes""#,
                Some("yes"),
            ),
            (r#"Bearer x_resource_metadata="no""#, None),
            (r#"Bearer resource_metadata_uri="no""#, None),
            ("Bearer resource_metadata", None),
            ("Bearer", None),
            ("", None),
        ] {
            assert_eq!(named(header), value, "{header}");
        }
    }

    fn origin() -> Uri {
        "https://mcp.example".parse().unwrap()
    }

    #[test]
    fn a_named_url_is_fetched_only_on_this_origin_and_under_well_known() {
        let challenge = |url: &str| format!(r#"Bearer resource_metadata="{url}""#);
        let plan = |url: &str| plan(Some(&challenge(url)), &origin(), "/mcp");
        assert_eq!(
            plan("https://MCP.example/.well-known/oauth-protected-resource/mcp?x=1"),
            Plan::Named("/.well-known/oauth-protected-resource/mcp".to_owned())
        );
        for other in [
            "http://mcp.example/.well-known/x",
            "https://other.example/.well-known/x",
            "https://mcp.example:444/.well-known/x",
            "/.well-known/x",
        ] {
            assert_eq!(
                plan(other),
                Plan::Elsewhere("it is on another origin than the URL given"),
                "{other}"
            );
        }
        for outside in [
            "https://mcp.example/metadata",
            "https://mcp.example/.well-known/",
        ] {
            assert_eq!(
                plan(outside),
                Plan::Elsewhere("it is not under /.well-known/"),
                "{outside}"
            );
        }
        assert_eq!(plan("not a url"), Plan::Elsewhere("it is not a URL"));
    }

    #[test]
    fn with_no_url_named_the_well_known_locations_are_the_endpoints_then_the_roots() {
        let well_known =
            |challenge: Option<&str>, endpoint: &str| match plan(challenge, &origin(), endpoint) {
                Plan::WellKnown(paths) => paths,
                other => panic!("{other:?}"),
            };
        let root = "/.well-known/oauth-protected-resource";
        assert_eq!(
            well_known(None, "/mcp?key=1"),
            [format!("{root}/mcp"), root.to_owned()]
        );
        assert_eq!(
            well_known(Some("Bearer scope=\"a\""), "/a/b/"),
            [format!("{root}/a/b"), root.to_owned()]
        );
        // An endpoint at the root has one location.
        assert_eq!(well_known(None, "/"), [root.to_owned()]);
        assert_eq!(well_known(None, "/?k=1"), [root.to_owned()]);
        assert_eq!(well_known(None, ""), [root.to_owned()]);
    }
}
