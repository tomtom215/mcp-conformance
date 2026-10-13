// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The front end, compiled into the binary: one HTML page, its stylesheets
//! and its ES modules (ADR-0021, "The front end": no framework, no bundler,
//! no build output). Each is a file under `ui/`, served from `/assets/` so
//! the Content Security Policy can admit `'self'` and nothing inline.
//!
//! `report.css` is not here: it is the HTML report's own stylesheet
//! ([`mcp_trace_validator::html::STYLESHEET`]), served as it is, so the
//! session timeline looks like the report because it is styled by it.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;

use super::guard::plain;

/// The page.
pub(crate) const PAGE: &str = include_str!("ui/index.html");

/// Every asset: its name under `/assets/`, its media type, and its text.
pub(crate) const ASSETS: [(&str, &str, &str); 12] = [
    ("icon.svg", "image/svg+xml", include_str!("ui/icon.svg")),
    (
        "report.css",
        "text/css; charset=utf-8",
        mcp_trace_validator::html::STYLESHEET,
    ),
    (
        "workbench.css",
        "text/css; charset=utf-8",
        include_str!("ui/workbench.css"),
    ),
    (
        "app.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/app.js"),
    ),
    (
        "dom.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/dom.js"),
    ),
    (
        "library.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/library.js"),
    ),
    (
        "admin.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/admin.js"),
    ),
    (
        "session.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/session.js"),
    ),
    (
        "inspector.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/inspector.js"),
    ),
    (
        "diff.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/diff.js"),
    ),
    (
        "dashboard.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/dashboard.js"),
    ),
    (
        "charts.js",
        "text/javascript; charset=utf-8",
        include_str!("ui/charts.js"),
    ),
];

fn respond(media: &'static str, text: &'static str) -> Response {
    let mut response = Response::new(Body::from(text));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(media));
    response
}

/// The page itself.
pub(crate) fn page() -> Response {
    respond("text/html; charset=utf-8", PAGE)
}

/// The asset `name`, or 404.
pub(crate) fn asset(name: &str) -> Response {
    ASSETS
        .iter()
        .find(|(asset, _, _)| *asset == name)
        .map_or_else(
            || plain(StatusCode::NOT_FOUND, "not found\n"),
            |(_, media, text)| respond(media, text),
        )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// The rule that keeps trace content inert, held mechanically: no script
    /// the workbench ships may parse a string as markup or code. Text reaches
    /// the page only as text nodes (`dom.js`), whatever the recording holds.
    #[test]
    fn no_script_turns_a_string_into_markup_or_code() {
        for (name, _, text) in ASSETS.iter().filter(|(name, _, _)| {
            Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
        }) {
            for sink in [
                "innerHTML",
                "outerHTML",
                "insertAdjacentHTML",
                "document.write",
                "eval(",
                "new Function",
                "setTimeout(\"",
                "createContextualFragment",
                "DOMParser",
                "srcdoc",
            ] {
                assert!(!text.contains(sink), "{name} uses {sink}");
            }
        }
        // And the page has no inline script or style for the policy to refuse.
        assert!(!PAGE.contains("<script>") && !PAGE.contains("style="));
        assert!(!PAGE.contains("<style"));
    }

    #[test]
    fn every_asset_the_page_and_modules_name_is_served() {
        for (name, _, _) in ASSETS {
            assert_eq!(asset(name).status(), StatusCode::OK, "{name}");
        }
        assert_eq!(asset("nope.js").status(), StatusCode::NOT_FOUND);
        for reference in PAGE.split("/assets/").skip(1) {
            let name = reference.split('"').next().unwrap_or_default();
            assert!(ASSETS.iter().any(|(asset, _, _)| *asset == name), "{name}");
        }
        for (owner, _, text) in ASSETS.iter().filter(|(name, _, _)| {
            Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
        }) {
            for reference in text.split("from './").skip(1) {
                let name = reference.split('\'').next().unwrap_or_default();
                assert!(
                    ASSETS.iter().any(|(asset, _, _)| *asset == name),
                    "{owner} imports {name}"
                );
            }
        }
        assert_eq!(page().status(), StatusCode::OK);
    }
}
