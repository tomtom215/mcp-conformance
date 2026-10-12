// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::relay::{End, read_prefix};
use super::target::{is_event_stream, location, target};
use super::*;
use axum::body::Bytes;
use axum::http::uri::PathAndQuery;
use axum::http::{HeaderMap, header};

/// `read_prefix` over `chunks` with a limit of 1024.
fn prefix_of(chunks: &[usize]) -> (usize, bool) {
    let chunks: Vec<Result<Bytes, ()>> = chunks
        .iter()
        .map(|&len| Ok(Bytes::from(vec![b'x'; len])))
        .collect();
    let mut stream = futures::stream::iter(chunks);
    let prefix = futures::executor::block_on(read_prefix(&mut stream, 1024));
    (prefix.bytes.len(), matches!(prefix.end, End::Complete))
}

#[test]
fn a_body_that_fails_keeps_what_was_read() {
    let chunks: Vec<Result<Bytes, &str>> = vec![
        Ok(Bytes::from_static(b"{\"a\":")),
        Err("cut"),
        Ok(Bytes::new()),
    ];
    let mut stream = futures::stream::iter(chunks);
    let prefix = futures::executor::block_on(read_prefix(&mut stream, 1024));
    assert_eq!(prefix.bytes, b"{\"a\":");
    assert!(matches!(prefix.end, End::Failed("cut")));
}

#[test]
fn a_body_is_whole_up_to_the_limit_and_cut_as_soon_as_it_passes() {
    assert_eq!(prefix_of(&[1024]), (1024, true));
    assert_eq!(prefix_of(&[512, 512]), (1024, true));
    assert_eq!(prefix_of(&[1025]), (1025, false));
    // A chunk that jumps past the limit stops the read there: the rest of the
    // body is streamed on, not buffered.
    assert_eq!(prefix_of(&[600, 600, 600]), (1200, false));
}

#[test]
fn target_appends_the_request_path_to_the_upstream_path() {
    let pq = |text: &str| text.parse::<PathAndQuery>().unwrap();
    let root: Uri = "http://localhost:3000".parse().unwrap();
    assert_eq!(
        target(&root, Some(&pq("/mcp?x=1"))).to_string(),
        "http://localhost:3000/mcp?x=1"
    );
    let prefixed: Uri = "https://example.com/api/".parse().unwrap();
    assert_eq!(
        target(&prefixed, Some(&pq("/mcp"))).to_string(),
        "https://example.com/api/mcp"
    );
    assert_eq!(target(&root, None).to_string(), "http://localhost:3000/");
}

/// The proxy's root is the upstream URL itself: appending `/` sent a client
/// pointed at the proxy to `/mcp/`, which servers answer with a redirect to
/// `/mcp` — so no client URL reached exactly `/mcp`.
#[test]
fn the_root_path_is_the_upstream_itself() {
    let pq = |text: &str| text.parse::<PathAndQuery>().unwrap();
    let endpoint: Uri = "http://h:1/mcp".parse().unwrap();
    assert_eq!(
        target(&endpoint, Some(&pq("/"))).to_string(),
        "http://h:1/mcp"
    );
    assert_eq!(target(&endpoint, None).to_string(), "http://h:1/mcp");
    assert_eq!(
        target(&endpoint, Some(&pq("/?x=1"))).to_string(),
        "http://h:1/mcp?x=1"
    );
    assert_eq!(
        target(&endpoint, Some(&pq("/x"))).to_string(),
        "http://h:1/mcp/x"
    );
    let slashed: Uri = "http://h:1/mcp/".parse().unwrap();
    assert_eq!(
        target(&slashed, Some(&pq("/"))).to_string(),
        "http://h:1/mcp/"
    );
}

/// A redirect into the upstream is pointed back through the proxy, by the
/// inverse of the request mapping; anything it cannot reach is left alone.
#[test]
fn a_redirect_into_the_upstream_comes_back_through_the_proxy() {
    let endpoint: Uri = "http://h:1/mcp".parse().unwrap();
    let back = |sent: &str| location(&endpoint, sent, Some("proxy:8"));
    assert_eq!(back("http://h:1/mcp").as_deref(), Some("http://proxy:8/"));
    assert_eq!(
        back("HTTP://H:1/mcp/x?y=1").as_deref(),
        Some("http://proxy:8/x?y=1")
    );
    assert_eq!(back("/mcp/x").as_deref(), Some("/x"), "a path stays a path");
    assert_eq!(back("/mcp").as_deref(), Some("/"));
    // Unreachable through the proxy, or not the upstream at all.
    assert_eq!(back("/mcp/"), None, "the proxy's / is /mcp, not /mcp/");
    assert_eq!(back("/other"), None);
    assert_eq!(back("/mcpx"), None);
    assert_eq!(back("http://h:2/mcp"), None);
    assert_eq!(back("https://h:1/mcp"), None);
    assert_eq!(back("//h:1/mcp"), None);
    assert_eq!(back("mcp"), None);
    assert_eq!(back("not a uri"), None);
    assert_eq!(
        location(&endpoint, "http://h:1/mcp", None),
        None,
        "no Host to point the client back by"
    );
    assert_eq!(
        location(&endpoint, "/mcp/x", None).as_deref(),
        Some("/x"),
        "a path needs no Host"
    );

    let root: Uri = "http://h:1".parse().unwrap();
    assert_eq!(location(&root, "/mcp", None).as_deref(), Some("/mcp"));
    assert_eq!(location(&root, "/", None).as_deref(), Some("/"));

    // The upstream's own query is prepended to every request's, so only a
    // query carrying it maps back, without it.
    let keyed: Uri = "http://h:1/mcp?key=k".parse().unwrap();
    let back = |sent: &str| location(&keyed, sent, Some("p"));
    assert_eq!(back("/mcp?key=k").as_deref(), Some("/"));
    assert_eq!(back("/mcp/x?key=k&y=1").as_deref(), Some("/x?y=1"));
    assert_eq!(back("/mcp?key=other"), None);
    assert_eq!(back("/mcp?key=kk"), None);
    assert_eq!(back("/mcp"), None);
}

#[test]
fn only_absolute_http_upstreams_are_accepted() {
    let uri = |text: &str| text.parse::<Uri>().unwrap();
    assert!(Options::new(uri("http://127.0.0.1:3000"), 1).is_ok());
    assert_eq!(
        Options::new(uri("https://example.com"), 1).is_ok(),
        cfg!(feature = "tls")
    );
    assert!(Options::new(uri("/relative"), 1).is_err());
    assert!(Options::new(uri("ftp://example.com"), 1).is_err());
}

#[test]
fn event_streams_are_recognised_with_parameters_and_any_case() {
    let with = |value: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, value.parse().unwrap());
        headers
    };
    assert!(is_event_stream(&with("text/event-stream")));
    assert!(is_event_stream(&with("Text/Event-Stream; charset=utf-8")));
    assert!(!is_event_stream(&with("application/json")));
    assert!(!is_event_stream(&HeaderMap::new()));
}

#[test]
fn the_upstreams_query_is_kept_and_the_requests_appended() {
    let pq = |text: &str| text.parse::<PathAndQuery>().unwrap();
    let keyed: Uri = "http://h:1/base?key=k".parse().unwrap();
    assert_eq!(
        target(&keyed, Some(&pq("/mcp?x=1"))).to_string(),
        "http://h:1/base/mcp?key=k&x=1"
    );
    assert_eq!(
        target(&keyed, Some(&pq("/mcp"))).to_string(),
        "http://h:1/base/mcp?key=k"
    );
}

#[test]
fn an_upstream_with_credentials_is_refused_without_repeating_them() {
    let uri = |text: &str| text.parse::<Uri>().unwrap();
    let error = Options::new(uri("http://user:hunter2@example.com/mcp"), 1).unwrap_err();
    assert!(!error.contains("hunter2"), "{error}");
    assert!(error.contains("credentials"), "{error}");
    assert!(Options::new(uri("http://user@example.com"), 1).is_err());
}
