// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The request path's two bounds on what a page elsewhere on this host can
//! make the workbench do: the API answers only its own page and the user's
//! own navigation, and an export is as large as the library, never larger.

#![allow(clippy::unwrap_used)]

use std::time::Instant;

use super::tests::{send, shared};
use super::*;
use crate::workbench::ops::{EXPORT_MAX_ENTRIES, export_unique};
use crate::workbench::tar;

const HOST: &str = "127.0.0.1:4200";

/// A `GET` with the right `Host` and the cookie, and `Sec-Fetch-Site: site`
/// when one is given — as a browser sends it.
fn fetch(shared: &Shared, path: &str, site: Option<&'static str>) -> Request<Body> {
    let mut builder = Request::get(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, shared.keys.cookie());
    if let Some(site) = site {
        builder = builder.header("sec-fetch-site", site);
    }
    builder.body(Body::empty()).unwrap()
}

/// Another server on `127.0.0.1` — another port — is *same-site* to the
/// browser, so a page it serves can send a credentialed `GET` (an `<img>`,
/// a link) without any `Origin`. The API refuses it; the page itself, which
/// only shows what the API answers, is still reachable from such a link.
#[tokio::test]
async fn the_api_answers_only_its_own_page_and_the_users_own_navigation() {
    let (shared, _dir) = shared("fetch-site");
    for path in [
        "/api/library",
        "/api/stats",
        "/api/export?ids=everything-http",
    ] {
        for site in ["same-site", "cross-site", "unknown-value"] {
            let (status, _, _) = send(&shared, fetch(&shared, path, Some(site))).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path} under {site}");
        }
        for site in [Some("same-origin"), Some("none"), None] {
            let (status, _, _) = send(&shared, fetch(&shared, path, site)).await;
            assert_eq!(status, StatusCode::OK, "{path} under {site:?}");
        }
    }
    for path in ["/", "/assets/app.js"] {
        let (status, _, _) = send(&shared, fetch(&shared, path, Some("same-site"))).await;
        assert_eq!(status, StatusCode::OK, "{path} is not the API");
    }
}

/// `ids=` named one entry 2,000 times and got it 2,000 times, in memory: the
/// archive holds each named entry once, in the order first named.
#[tokio::test]
async fn an_export_holds_each_entry_once_in_first_named_order() {
    let (shared, _dir) = shared("export-once");
    let repeated = "sdk-python-2025-11-25,".repeat(2_000);
    let started = Instant::now();
    let (status, _, body) = send(
        &shared,
        fetch(
            &shared,
            &format!("/api/export?ids={repeated}tool-005-name-length,sdk-python-2025-11-25"),
            None,
        ),
    )
    .await;
    let took = started.elapsed();
    assert_eq!(status, StatusCode::OK);
    let names: Vec<String> = tar::tests::read(&body)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        names,
        [
            "sdk-python-2025-11-25.jsonl",
            "sdk-python-2025-11-25.meta.json",
            "tool-005-name-length.jsonl",
            "tool-005-name-length.meta.json",
        ],
        "{} bytes in {took:?}",
        body.len()
    );
    // Two entries with their sidecars (a sidecar carries the verdict cache,
    // ~110 KB for this one): well under a megabyte, where 2,000 copies were
    // 261 MB.
    assert!(body.len() < 1024 * 1024, "{} bytes in {took:?}", body.len());
    assert_eq!(
        export_unique(&["b", "a", "b", "c", "a"]),
        ["b", "a", "c"],
        "first-named order"
    );
}

/// More entries than one export holds is refused before the library is read
/// — with the bound named, whether or not the entries exist.
#[tokio::test]
async fn an_export_of_more_entries_than_the_bound_is_refused_whole() {
    let (shared, _dir) = shared("export-bound");
    let too_many: Vec<String> = (0..=EXPORT_MAX_ENTRIES).map(|n| format!("e{n}")).collect();
    let (status, _, body) = send(
        &shared,
        fetch(
            &shared,
            &format!("/api/export?ids={}", too_many.join(",")),
            None,
        ),
    )
    .await;
    let answer: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    let why = answer["error"].as_str().unwrap();
    assert!(
        why.contains(&format!("at most {EXPORT_MAX_ENTRIES}"))
            && why.contains(&(EXPORT_MAX_ENTRIES + 1).to_string()),
        "{why}"
    );
    // Exactly the bound, every id the same entry: one entry, well within it.
    let at_bound = "everything-http,".repeat(EXPORT_MAX_ENTRIES);
    let (status, _, _) = send(
        &shared,
        fetch(&shared, &format!("/api/export?ids={at_bound}"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The bound holds for the command line too: the operation is shared.
    let names: Vec<&str> = too_many.iter().map(String::as_str).collect();
    let refused = crate::workbench::ops::export_archive(&shared.lock(), &names).unwrap_err();
    assert!(matches!(refused, Failure::Invalid(why) if why.contains("at most")));
    assert_eq!(
        send(&shared, fetch(&shared, "/api/export?ids=nope", None))
            .await
            .0,
        StatusCode::NOT_FOUND,
        "within the bound, an unknown entry is still not found"
    );
}
