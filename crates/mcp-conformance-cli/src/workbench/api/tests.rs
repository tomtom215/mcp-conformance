// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The server in process: every defense on the request path, and every API,
//! over a copy of the fixture library.

#![allow(clippy::unwrap_used)]

use axum::body::to_bytes;
use axum::http::Method;

use super::*;
use crate::workbench::fixture;
use crate::workbench::server;

const HOST: &str = "127.0.0.1:4200";
const ORIGIN: &str = "http://127.0.0.1:4200";

fn shared(name: &str) -> (Arc<Shared>, fixture::Scratch) {
    let dir = fixture::library(name);
    (server::shared(&dir, HOST.parse().unwrap()).unwrap(), dir)
}

/// A request as the page sends it: the right Host, the cookie, and for a
/// change, its Origin and a JSON body.
fn request(shared: &Shared, method: Method, path: &str, body: Option<&Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, shared.keys.cookie());
    if let Some(body) = body {
        builder = builder
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json");
        return builder.body(Body::from(body.to_string())).unwrap();
    }
    builder.body(Body::empty()).unwrap()
}

async fn send(
    shared: &Arc<Shared>,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = handle(shared.clone(), request).await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, body)
}

async fn get(shared: &Arc<Shared>, path: &str) -> (StatusCode, Value) {
    let (status, _, body) = send(shared, request(shared, Method::GET, path, None)).await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn post(shared: &Arc<Shared>, path: &str, body: Value) -> (StatusCode, Value) {
    let (status, _, body) = send(shared, request(shared, Method::POST, path, Some(&body))).await;
    (status, serde_json::from_slice(&body).unwrap())
}

fn hardened(headers: &axum::http::HeaderMap) {
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        guard::CONTENT_SECURITY_POLICY
    );
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(headers.get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn a_request_without_the_cookie_is_refused_and_still_hardened() {
    let (shared, _dir) = shared("unauthenticated");
    for path in ["/", "/assets/app.js", "/api/library", "/api/stats"] {
        let request = Request::get(path)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap();
        let (status, headers, _) = send(&shared, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        hardened(&headers);
    }
}

#[tokio::test]
async fn the_launch_link_sets_the_cookie_once_and_redirects_away_from_itself() {
    let (shared, _dir) = shared("launch");
    let link = format!("/?token={}", shared.keys.launch_token());
    let launch = || {
        Request::get(link.as_str())
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap()
    };
    let (status, headers, _) = send(&shared, launch()).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/");
    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with(&shared.keys.cookie()));
    hardened(&headers);
    let (status, _, _) = send(&shared, launch()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The browser that holds the cookie, reloading the link, is sent on.
    let reload = request(&shared, Method::GET, &link, None);
    let (status, headers, _) = send(&shared, reload).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(headers.get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn a_rebound_host_or_a_foreign_origin_is_refused_even_with_the_cookie() {
    let (shared, _dir) = shared("origin");
    let mut rebound = request(&shared, Method::GET, "/api/library", None);
    rebound.headers_mut().insert(
        header::HOST,
        HeaderValue::from_static("attacker.example:4200"),
    );
    let (status, headers, _) = send(&shared, rebound).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    hardened(&headers);
    let mut cross = request(&shared, Method::GET, "/api/library", None);
    cross.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://attacker.example"),
    );
    let (status, headers, _) = send(&shared, cross).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(headers.get("access-control-allow-origin").is_none());
    // A change from a page elsewhere: no Origin of ours.
    let mut forged = request(
        &shared,
        Method::POST,
        "/api/sessions/everything-http/delete",
        Some(&json!({})),
    );
    forged.headers_mut().remove(header::ORIGIN);
    assert_eq!(send(&shared, forged).await.0, StatusCode::FORBIDDEN);
    // A form post cannot send JSON.
    let mut form = request(
        &shared,
        Method::POST,
        "/api/sessions/everything-http/delete",
        Some(&json!({})),
    );
    form.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    assert_eq!(
        send(&shared, form).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert!(
        shared.lock().entries.contains_key("everything-http"),
        "nothing was deleted"
    );
}

#[tokio::test]
async fn the_page_and_its_assets_are_served_with_their_types() {
    let (shared, _dir) = shared("assets");
    let (status, headers, body) = send(&shared, request(&shared, Method::GET, "/", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert!(String::from_utf8(body).unwrap().contains("/assets/app.js"));
    hardened(&headers);
    for (name, media) in [
        ("app.js", "text/javascript"),
        ("report.css", "text/css"),
        ("icon.svg", "image/svg+xml"),
    ] {
        let (status, headers, _) = send(
            &shared,
            request(&shared, Method::GET, &format!("/assets/{name}"), None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with(media),
            "{name}"
        );
    }
    assert_eq!(
        send(&shared, request(&shared, Method::GET, "/assets/x.js", None))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&shared, request(&shared, Method::GET, "/nope", None))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(get(&shared, "/api/nope").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        post(&shared, "/api/nope", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    let put = request(&shared, Method::PUT, "/", None);
    assert_eq!(
        send(&shared, put).await.0,
        StatusCode::FORBIDDEN,
        "PUT needs an Origin"
    );
}

#[tokio::test]
async fn a_hostile_recording_reaches_the_page_as_json_strings_only() {
    let (shared, _dir) = shared("hostile");
    let (status, headers, body) = send(
        &shared,
        request(&shared, Method::GET, "/api/sessions/hostile-markup", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    hardened(&headers);
    let view: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        view["summary"]["facts"]["client"],
        "<script>alert(1)</script> 0"
    );
    assert!(
        view["events"][5]["payload"]
            .as_str()
            .unwrap()
            .contains("<img src=x onerror=alert(5)>")
    );
}

#[tokio::test]
async fn reads_answer_from_the_library() {
    let (shared, dir) = shared("reads");
    let (_, library) = get(&shared, "/api/library").await;
    assert_eq!(library["sessions"].as_array().unwrap().len(), 11);
    assert_eq!(library["library"], dir.display().to_string());
    let (_, found) = get(&shared, "/api/search?q=req%3ABASE-001+verdict:fail").await;
    assert_eq!(found["hits"][0]["id"], "base-001-request-id-boolean");
    let (_, bad) = get(&shared, "/api/search?q=%22open").await;
    assert!(bad["error"].as_str().unwrap().contains("not closed"));
    let (_, noted) = get(&shared, "/api/search?q=https://x").await;
    assert_eq!(noted["notes"].as_array().unwrap().len(), 1);
    let (_, page) = get(&shared, "/api/sessions/sdk-go-2025-11-25?from=30").await;
    assert_eq!(page["events"][0]["seq"], 30);
    let (_, diff) = get(
        &shared,
        "/api/diff?a=sdk-go-2025-11-25&b=sdk-rust-2025-11-25",
    )
    .await;
    assert_eq!(diff["clauses"][0]["id"], "RES-006");
    assert_eq!(
        get(&shared, "/api/sessions/..%2Fx").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&shared, "/api/sessions/missing").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&shared, "/api/diff?a=missing&b=everything-http")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_dashboards_read_exactly_what_library_stats_prints() {
    let (shared, dir) = shared("stats");
    let (status, headers, body) =
        send(&shared, request(&shared, Method::GET, "/api/stats", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    let mut workbench = Workbench::open(&dir).unwrap();
    let printed = crate::library_cmd::execute(
        &mut workbench,
        crate::args::LibraryCommand::Stats {
            format: crate::args::StatsFormat::Json,
        },
    )
    .unwrap();
    assert_eq!(String::from_utf8(body).unwrap(), printed);
}

#[tokio::test]
async fn the_report_downloads_as_the_validators_page() {
    let (shared, _dir) = shared("report");
    let (status, headers, body) = send(
        &shared,
        request(
            &shared,
            Method::GET,
            "/api/sessions/base-001-request-id-boolean/report.html",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        guard::REPORT_POLICY
    );
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    let page = String::from_utf8(body).unwrap();
    assert!(page.starts_with("<!DOCTYPE html>") && page.contains("BASE-001"));
    // One that could not be judged has no report.
    std::fs::write(shared.lock().library.trace_path("broken"), "x\n").unwrap();
    assert_eq!(
        get(&shared, "/api/sessions/broken/report.html").await.0,
        StatusCode::BAD_REQUEST
    );
}

// One scenario, step by step; each step reads the state the last left.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn changes_import_tag_redact_delete_and_expire() {
    let (shared, dir) = shared("changes");
    let capture = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance/mcpsnoop/native/stdio-2025-11-25.mcpsnoop.jsonl"),
    )
    .unwrap();
    let (status, entry) = post(
        &shared,
        "/api/import",
        json!({"name": "capture.jsonl", "text": capture}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{entry}");
    assert_eq!(entry["id"], "capture");
    assert_eq!(entry["source"]["importer"], "mcpsnoop");
    assert_ne!(entry["source"]["not_recorded"], json!([]));
    let (status, _) = post(
        &shared,
        "/api/import",
        json!({"name": "x.json", "text": "[]"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(
        &shared,
        "/api/import",
        json!({"name": "x", "format": "pcap", "text": ""}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(
        &shared,
        "/api/import",
        json!({"name": "x", "transport": "carrier-pigeon", "text": capture}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, tagged) = post(
        &shared,
        "/api/sessions/capture/tags",
        json!({"tags": ["b", "a"]}),
    )
    .await;
    assert_eq!(tagged["tags"], json!(["a", "b"]));

    let (status, copy) = post(
        &shared,
        "/api/sessions/leaky-session/redact",
        json!({"meta": true, "strings": ["fixture-api-key-3"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{copy}");
    let id = copy["id"].as_str().unwrap();
    let text = std::fs::read_to_string(dir.join(format!("{id}.jsonl"))).unwrap();
    for secret in [
        "fixture-not-a-real-token-1",
        "fixture-cookie-2",
        "fixture-api-key-3",
        "fixture-meta-4",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert_eq!(copy["source"]["redacted_from"], "leaky-session");
    assert_eq!(
        post(
            &shared,
            "/api/sessions/leaky-session/redact",
            json!({"pointers": ["x"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(
            &shared,
            "/api/sessions/leaky-session/redact",
            json!({"unknown": 1})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let (_, deleted) = post(&shared, "/api/sessions/capture/delete", json!({})).await;
    assert_eq!(deleted["deleted"], json!(["capture"]));
    assert!(!dir.join("capture.jsonl").exists());

    let (_, listed) = post(&shared, "/api/retention", json!({"days": 0})).await;
    assert_eq!(listed["applied"], false);
    let before = listed["entries"].as_array().unwrap().len();
    assert!(before >= 11, "every fixture is older than today");
    let (_, kept) = post(&shared, "/api/retention", json!({"days": 100_000})).await;
    assert_eq!(kept["entries"], json!([]));
    let (_, applied) = post(&shared, "/api/retention", json!({"days": 0, "apply": true})).await;
    assert_eq!(applied["entries"].as_array().unwrap().len(), before);
    assert!(shared.lock().entries.len() < 12);
}

#[tokio::test]
async fn an_export_is_a_tar_of_the_traces_and_sidecars() {
    let (shared, _dir) = shared("export");
    let (status, headers, body) = send(
        &shared,
        request(
            &shared,
            Method::GET,
            "/api/export?ids=everything-http,tool-005-name-length",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-tar");
    let members = crate::workbench::tar::tests::read(&body);
    let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "everything-http.jsonl",
            "everything-http.meta.json",
            "tool-005-name-length.jsonl",
            "tool-005-name-length.meta.json"
        ]
    );
    assert_eq!(
        get(&shared, "/api/export?ids=").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&shared, "/api/export?ids=..").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn query_components_are_percent_decoded() {
    assert_eq!(decode("a+b%3Ac%2f%E2%9C%93"), "a b:c/✓");
    assert_eq!(decode("100%"), "100%");
    assert_eq!(decode("%zz%4"), "%zz%4");
}
