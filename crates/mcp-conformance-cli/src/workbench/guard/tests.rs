// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Each defense on its own; `api/tests.rs` drives them through the server.

#![allow(clippy::unwrap_used)]

use super::*;

fn keys() -> Keys {
    Keys::new("127.0.0.1:4100".parse().unwrap()).unwrap()
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(*name, HeaderValue::from_str(value).unwrap());
    }
    map
}

#[test]
fn secrets_are_random_and_distinct() {
    let (a, b) = (secret().unwrap(), secret().unwrap());
    assert_eq!(a.len(), 64);
    assert!(a.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(a, b);
    let keys = keys();
    assert_ne!(
        keys.launch, keys.session,
        "the link never carries the cookie's secret"
    );
    assert!(
        keys.launch_url()
            .starts_with("http://127.0.0.1:4100/?token=")
    );
}

#[test]
fn host_must_be_the_bound_address() {
    let keys = keys();
    for host in ["127.0.0.1:4100", "localhost:4100"] {
        assert!(
            keys.check_origin(&Method::GET, &headers(&[("host", host)]))
                .is_ok(),
            "{host}"
        );
    }
    for host in [
        "evil.example:4100",
        "127.0.0.1:4101",
        "127.0.0.1",
        "localhost",
        "[::1]:4100",
        "127.0.0.1:4100.evil",
    ] {
        let refused = keys
            .check_origin(&Method::GET, &headers(&[("host", host)]))
            .unwrap_err();
        assert_eq!(refused.0, StatusCode::FORBIDDEN, "{host}");
    }
    assert!(
        keys.check_origin(&Method::GET, &HeaderMap::new()).is_err(),
        "no Host"
    );
    let v6 = Keys::new("[::1]:9".parse().unwrap()).unwrap();
    assert!(
        v6.check_origin(&Method::GET, &headers(&[("host", "[::1]:9")]))
            .is_ok()
    );
    let wide = Keys::new("0.0.0.0:9".parse().unwrap()).unwrap();
    assert!(
        wide.check_origin(&Method::GET, &headers(&[("host", "localhost:9")]))
            .is_err(),
        "localhost only on loopback"
    );
}

#[test]
fn origin_must_be_the_workbench_and_a_change_must_carry_it() {
    let keys = keys();
    let host = ("host", "127.0.0.1:4100");
    for origin in ["http://127.0.0.1:4100", "http://localhost:4100"] {
        assert!(
            keys.check_origin(&Method::POST, &headers(&[host, ("origin", origin)]))
                .is_ok()
        );
    }
    for origin in [
        "http://evil.example",
        "http://127.0.0.1:4101",
        "https://127.0.0.1:4100",
        "null",
        "",
    ] {
        let refused = keys
            .check_origin(&Method::GET, &headers(&[host, ("origin", origin)]))
            .unwrap_err();
        assert!(refused.1.contains("cross-origin"), "{origin}");
    }
    assert!(
        keys.check_origin(&Method::POST, &headers(&[host]))
            .unwrap_err()
            .1
            .contains("needs the workbench's Origin")
    );
    assert!(
        keys.check_origin(&Method::DELETE, &headers(&[host]))
            .is_err()
    );
    assert!(keys.check_origin(&Method::HEAD, &headers(&[host])).is_ok());
    let cross = headers(&[host, ("sec-fetch-site", "cross-site")]);
    assert!(
        keys.check_origin(&Method::GET, &cross)
            .unwrap_err()
            .1
            .contains("cross-site")
    );
    let same = headers(&[host, ("sec-fetch-site", "same-origin")]);
    assert!(keys.check_origin(&Method::GET, &same).is_ok());
}

#[test]
fn the_launch_link_works_once_and_only_with_its_token() {
    let keys = keys();
    let uri = |text: &str| text.parse::<Uri>().unwrap();
    assert!(keys.launch(&uri("/")).is_none(), "no token, no exchange");
    assert!(keys.launch(&uri("/?a=b")).is_none());
    let wrong = keys.launch(&uri("/?token=00")).unwrap().unwrap_err();
    assert!(wrong.1.contains("not the launch link"));
    let link = format!("/?x=1&token={}", keys.launch_token());
    let cookie = keys.launch(&uri(&link)).unwrap().unwrap();
    assert!(cookie.starts_with(&keys.cookie()));
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
            && cookie.contains("Path=/")
    );
    let again = keys.launch(&uri(&link)).unwrap().unwrap_err();
    assert!(again.1.contains("has been used"));
}

#[test]
fn only_the_session_cookie_authenticates() {
    let keys = keys();
    assert!(!keys.authenticated(&HeaderMap::new()));
    assert!(keys.authenticated(&headers(&[("cookie", &keys.cookie())])));
    assert!(keys.authenticated(&headers(&[(
        "cookie",
        &format!("other=1; {}", keys.cookie())
    )])));
    assert!(keys.authenticated(&headers(&[("cookie", "a=b"), ("cookie", &keys.cookie())])));
    let wrong = format!("{}0", keys.cookie());
    assert!(!keys.authenticated(&headers(&[("cookie", &wrong)])));
    let launch = format!("mcp-workbench-4100={}", keys.launch_token());
    assert!(
        !keys.authenticated(&headers(&[("cookie", &launch)])),
        "the launch token is not the session"
    );
    let other_port = keys.cookie().replace("4100", "4101");
    assert!(!keys.authenticated(&headers(&[("cookie", &other_port)])));
}

#[test]
fn comparison_and_queries_are_exact() {
    assert!(same("abc", "abc"));
    assert!(!same("abc", "abd") && !same("abc", "ab") && !same("", "a"));
    // Two differences that are equal bit patterns must not cancel out.
    assert!(!same("ab", "ba") && !same("aabb", "bbaa"));
    let uri: Uri = "/x?a=1&token=t&b".parse().unwrap();
    assert_eq!(query(&uri, "token"), Some("t"));
    assert_eq!(query(&uri, "b"), None);
    assert_eq!(query(&"/x".parse().unwrap(), "a"), None);
}

#[test]
fn every_response_is_hardened() {
    let mut response = plain(StatusCode::OK, "x");
    harden(&mut response, CONTENT_SECURITY_POLICY);
    let headers = response.headers();
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        CONTENT_SECURITY_POLICY
    );
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
    assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
    assert!(
        !headers
            .keys()
            .any(|name| name.as_str().starts_with("access-control-"))
    );
    for directive in [
        "default-src 'none'",
        "script-src 'self'",
        "frame-ancestors 'none'",
        "connect-src 'self'",
    ] {
        assert!(CONTENT_SECURITY_POLICY.contains(directive), "{directive}");
    }
    assert!(!CONTENT_SECURITY_POLICY.contains("unsafe"));
    assert!(REPORT_POLICY.starts_with("sandbox"));
}
