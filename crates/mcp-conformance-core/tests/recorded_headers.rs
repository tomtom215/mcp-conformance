// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which headers a recorder keeps: the allowlists, and the one
//! `Authorization` value that is not a credential.

use mcp_conformance_core::trace::{
    PROBE_AUTHORIZATION, RECORDED_HEADER_PREFIXES, RECORDED_HEADERS, records_header,
};

#[test]
fn a_listed_name_is_kept_whatever_its_value() {
    for name in RECORDED_HEADERS {
        assert!(records_header(name, ""), "{name}");
        assert!(records_header(name, "anything"), "{name}");
    }
    for prefix in RECORDED_HEADER_PREFIXES {
        assert!(records_header(&format!("{prefix}region"), "eu"), "{prefix}");
    }
}

#[test]
fn an_unlisted_name_is_dropped_whatever_its_value() {
    for name in [
        "cookie",
        "x-api-key",
        "proxy-authorization",
        "set-cookie",
        "",
    ] {
        assert!(!records_header(name, "secret"), "{name}");
        // The probe value under another name is still another header.
        assert!(!records_header(name, PROBE_AUTHORIZATION), "{name}");
    }
    // A prefix is a prefix: the same text later in a name does not count.
    assert!(!records_header("x-mcp-param-region", "eu"));
}

#[test]
fn authorization_is_kept_only_as_the_whole_probe_value() {
    assert!(records_header("authorization", PROBE_AUTHORIZATION));
    for value in [
        "",
        "Bearer secret",
        "Bearer mcp-conformance-probe-invalid-token-2",
        " Bearer mcp-conformance-probe-invalid-token",
        "bearer mcp-conformance-probe-invalid-token",
        "Bearer mcp-conformance-probe-invalid-token, Bearer secret",
    ] {
        assert!(!records_header("authorization", value), "{value:?}");
    }
    // The constant is the scheme and the token, as it is sent.
    assert_eq!(
        PROBE_AUTHORIZATION,
        "Bearer mcp-conformance-probe-invalid-token"
    );
}

#[test]
fn a_path_is_kept_only_when_it_names_something_under_well_known() {
    use mcp_conformance_core::trace::{RECORDED_PATH_PREFIX, records_path};

    assert_eq!(RECORDED_PATH_PREFIX, "/.well-known/");
    for kept in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/x",
    ] {
        assert!(records_path(kept), "{kept}");
    }
    for dropped in [
        // The prefix alone names nothing.
        "/.well-known/",
        "/.well-known",
        "",
        "/",
        // The MCP endpoint, and anything that merely contains the prefix.
        "/mcp",
        "/mcp/.well-known/oauth-protected-resource",
        ".well-known/x",
        "/.well-knownx/y-long-enough-to-pass-a-length-check",
        "/a-path-longer-than-the-prefix-is",
    ] {
        assert!(!records_path(dropped), "{dropped:?}");
    }
}
