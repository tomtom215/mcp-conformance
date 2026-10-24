// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::body::{End, read_prefix};
use super::target::{is_event_stream, is_refused_path, location, target};
use super::*;
use axum::body::Bytes;
use axum::http::uri::PathAndQuery;
use axum::http::{HeaderMap, header};

/// A buffer budget no test here reaches.
fn unlimited() -> Arc<tokio::sync::Semaphore> {
    Arc::new(tokio::sync::Semaphore::new(1 << 30))
}

/// `read_prefix` over `chunks` with a limit of 1024.
fn prefix_of(chunks: &[usize]) -> (usize, bool) {
    let chunks: Vec<Result<Bytes, ()>> = chunks
        .iter()
        .map(|&len| Ok(Bytes::from(vec![b'x'; len])))
        .collect();
    let mut stream = futures::stream::iter(chunks);
    let prefix = futures::executor::block_on(read_prefix(&mut stream, 1024, &unlimited()));
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
    let prefix = futures::executor::block_on(read_prefix(&mut stream, 1024, &unlimited()));
    assert_eq!(prefix.bytes, b"{\"a\":");
    assert!(matches!(prefix.end, End::Failed("cut")));
    drop(prefix);
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
    // A scheme-relative reference names another server, even though its
    // path would map through a root upstream.
    assert_eq!(location(&root, "//elsewhere/mcp", None), None);

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

/// A dot segment is refused however it is spelled, and so is a path that is
/// not UTF-8 once decoded; a dot inside a name, an encoded character a path
/// may hold, and a parameter on a name are forwarded.
#[test]
fn dot_segments_are_found_however_they_are_spelled() {
    for path in [
        "/..",
        "/.",
        "/../x",
        "/a/./b",
        "/a/%2e%2e/b",
        "/a/%2E.",
        "/a/.%2e/",
        "/a/..%2fb",
        "/a/..%2Fb",
        "/a\\..\\b",
        "/a/..%5cb",
        "/%2e",
        // With `;parameters`, which some servers strip before resolving.
        "/..;/secret",
        "/a/.;v=1/b",
        "/%2e%2e;x/secret",
        "/..%3b/x",
        // Not UTF-8 once decoded: an overlong `..`, a lone continuation byte.
        "/%c0%ae%c0%ae/secret",
        "/a%ff/b",
        "/%80",
    ] {
        assert!(is_refused_path(path), "{path}");
    }
    for path in [
        "/",
        "/mcp",
        "/mcp/",
        "/a/..b",
        "/.well-known/x",
        "/a/b..",
        "/a/%2e%2e%2e",
        "/a.b/c",
        "/a%20b",
        "/mcp;v=1",
        "/;",
        "/%e2%9c%93",
        // A `%` that encodes nothing is a literal; a doubly encoded dot is
        // the name `%2e`, which the proxy forwards as any other.
        "/a%zz/b%2",
        "/%252e%252e/x",
    ] {
        assert!(!is_refused_path(path), "{path}");
    }
}

/// The decoder reads `%XX` in either case and leaves a `%` that encodes
/// nothing — a non-hex digit, or too few of them — as the byte it is.
#[test]
fn percent_decoding_reads_each_escape_once_and_keeps_what_is_not_one() {
    use super::target::percent_decoded;
    assert_eq!(percent_decoded(b"/a%20b%2Fc%2fd"), b"/a b/c/d");
    assert_eq!(percent_decoded(b"%c0%AE"), [0xC0, 0xAE]);
    assert_eq!(percent_decoded(b"%zz%2%"), b"%zz%2%");
    assert_eq!(percent_decoded(b"%252e"), b"%2e");
    assert_eq!(percent_decoded(b""), b"");
}

/// A body that would take more of the buffer budget than is left stops being
/// read there — streamed on, not waited for — and what it took is given back
/// when its prefix is dropped.
#[test]
fn a_body_past_the_budget_stops_there_and_gives_the_budget_back() {
    let budget = Arc::new(tokio::sync::Semaphore::new(1000));
    let chunks = || {
        futures::stream::iter(
            [600, 600, 600]
                .map(|len| Ok::<_, ()>(Bytes::from(vec![b'x'; len])))
                .to_vec(),
        )
    };
    let mut first = chunks();
    let held = futures::executor::block_on(read_prefix(&mut first, 1 << 20, &budget));
    assert!(matches!(held.end, End::OverBudget));
    assert_eq!(
        held.bytes.len(),
        1200,
        "the chunk that did not fit is kept, not lost"
    );
    assert_eq!(budget.available_permits(), 400);
    // While the first holds its 600, a second body cannot have 600 more.
    let mut second = chunks();
    let refused = futures::executor::block_on(read_prefix(&mut second, 1 << 20, &budget));
    assert!(matches!(refused.end, End::OverBudget));
    assert_eq!(refused.bytes.len(), 600);
    drop((held, refused));
    assert_eq!(budget.available_permits(), 1000);
    // With the budget back, a body within it is read whole.
    let mut third = futures::stream::iter(vec![Ok::<_, ()>(Bytes::from(vec![b'x'; 900]))]);
    let whole = futures::executor::block_on(read_prefix(&mut third, 1 << 20, &budget));
    assert!(matches!(whole.end, End::Complete));
    assert_eq!(whole.held.bytes(), 900);
    assert_eq!(
        whole.bytes.capacity(),
        900,
        "what is held is what is allocated"
    );
    drop(whole);
    assert_eq!(budget.available_permits(), 1000);
}

/// What is held is what the buffer has allocated, chunk after chunk: a chunk
/// that fits in the capacity already held takes nothing more, and a growing
/// buffer holds exactly its new capacity.
#[test]
fn a_body_holds_exactly_its_buffers_capacity() {
    let budget = Arc::new(tokio::sync::Semaphore::new(1 << 20));
    let read = |chunks: &[usize]| {
        let mut stream = futures::stream::iter(
            chunks
                .iter()
                .map(|&len| Ok::<_, ()>(Bytes::from(vec![b'x'; len]))),
        );
        futures::executor::block_on(read_prefix(&mut stream, 1 << 16, &budget))
    };
    // An empty chunk at a full buffer grows nothing.
    let full = read(&[512, 0]);
    assert!(matches!(full.end, End::Complete));
    assert_eq!((full.bytes.capacity(), full.held.bytes()), (512, 512));
    drop(full);
    // A second chunk doubles the buffer, and the hold with it.
    let doubled = read(&[512, 512]);
    assert_eq!(
        (doubled.bytes.capacity(), doubled.held.bytes()),
        (1024, 1024)
    );
    drop(doubled);
    assert_eq!(budget.available_permits(), 1 << 20);
}

/// A hold grows from the budget, shrinks back into it, and — at the size it
/// already is — needs nothing from it at all.
#[test]
fn a_hold_resizes_both_ways() {
    let budget = Arc::new(tokio::sync::Semaphore::new(1000));
    let mut held = super::body::Held::default();
    assert!(held.resize(&budget, 100));
    assert_eq!((held.bytes(), budget.available_permits()), (100, 900));
    assert!(held.resize(&budget, 40));
    assert_eq!((held.bytes(), budget.available_permits()), (40, 960));
    // Unchanged needs no permit, so even a closed budget allows it.
    budget.close();
    assert!(held.resize(&budget, 40));
    assert_eq!(held.bytes(), 40);
    assert!(
        !held.resize(&budget, 41),
        "a closed budget gives nothing more"
    );
    assert!(held.resize(&budget, 0));
    assert_eq!((held.bytes(), budget.available_permits()), (0, 1000));
    drop(held);
    let mut none = super::body::Held::default();
    assert!(none.resize(&budget, 0));
    assert_eq!(none.bytes(), 0);
    drop(none);
}

/// A whole body is one frame of known length, and ends after it.
#[test]
fn a_whole_body_is_one_frame_then_its_end() {
    use hyper::body::Body as _;
    let mut body = super::body::whole(b"abc".to_vec(), super::body::Held::default());
    assert!(!body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(3));
    let waker = futures::task::noop_waker();
    let mut context = std::task::Context::from_waker(&waker);
    let std::task::Poll::Ready(Some(Ok(frame))) =
        std::pin::Pin::new(&mut body).poll_frame(&mut context)
    else {
        panic!("one frame");
    };
    assert_eq!(frame.into_data().unwrap(), Bytes::from_static(b"abc"));
    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
    assert!(super::body::whole(Vec::new(), super::body::Held::default()).is_end_stream());
}
