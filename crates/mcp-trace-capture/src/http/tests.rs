// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::relay::read_prefix;
use super::target::{is_event_stream, target};
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
    let (prefix, complete) = futures::executor::block_on(read_prefix(&mut stream, 1024)).unwrap();
    (prefix.len(), complete)
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
