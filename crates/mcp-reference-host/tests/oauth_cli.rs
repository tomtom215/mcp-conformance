// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `--oauth-conformance` through the real binary, against the real
//! everything-server behind a bearer check: the flag authorizes, its absence
//! does not, the suite's context chooses the client, and nothing the run
//! prints or records carries a token, a code or a secret.

#![cfg(all(feature = "cli", feature = "oauth"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::process::Command;

use support::serve;

/// Runs the host under the `initialize` plan; returns its exit code and stderr.
fn host(args: &[&str], context: Option<&str>) -> (Option<i32>, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"));
    command
        .env("MCP_CONFORMANCE_SCENARIO", "initialize")
        .env_remove("MCP_CONFORMANCE_CONTEXT")
        .args(args);
    if let Some(context) = context {
        command.env("MCP_CONFORMANCE_CONTEXT", context);
    }
    let output = command.output().expect("host runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_flag_authorizes_a_protected_server_and_the_run_completes() {
    let site = serve();
    let traces = std::env::temp_dir().join(format!("host-oauth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&traces);

    let (code, stderr) = host(
        &[
            "--oauth-conformance",
            "--trace-dir",
            traces.to_str().unwrap(),
            &site.mcp(),
        ],
        None,
    );

    assert_eq!(code, Some(0), "{stderr}");
    let asked = site.authorizations();
    assert_eq!(asked.len(), 1, "{stderr}");
    assert_eq!(asked[0]["client_id"], "registered-client");
    assert_eq!(asked[0]["code_challenge_method"], "S256");
    assert_eq!(site.token_requests().len(), 1);

    // What the run said and what it recorded carry no credential.
    let mut written = stderr;
    for entry in std::fs::read_dir(&traces).unwrap() {
        written.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap());
    }
    assert!(written.contains("\"initialize\""), "a trace was recorded");
    let exchanged = &site.token_requests()[0].form;
    for secret in [
        "token-1",
        "code-1",
        exchanged["code_verifier"].as_str(),
        asked[0]["state"].as_str(),
    ] {
        assert!(!written.contains(secret), "{secret} leaked: {written}");
    }
    let _ = std::fs::remove_dir_all(&traces);
}

#[test]
fn without_the_flag_the_challenge_is_not_answered() {
    let site = serve();
    let (code, stderr) = host(&[&site.mcp()], None);
    assert_eq!(code, Some(1), "{stderr}");
    assert_eq!(site.authorizations().len(), 0);
    assert_eq!(site.registrations(), 0);
}

#[test]
fn the_context_chooses_the_client_and_its_secret_is_never_printed() {
    let site = serve();
    let context = r#"{"name":"auth/pre-registration","client_id":"known-client","client_secret":"s3cr3t-value"}"#;

    let (code, stderr) = host(&["--oauth-conformance", &site.mcp()], Some(context));

    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(site.registrations(), 0);
    assert_eq!(site.authorizations()[0]["client_id"], "known-client");
    assert!(!stderr.contains("s3cr3t-value"), "{stderr}");
}

#[test]
fn a_context_that_cannot_be_used_is_an_invocation_error_that_does_not_echo_it() {
    let site = serve();
    let (code, stderr) = host(
        &["--oauth-conformance", &site.mcp()],
        Some(r#"["s3cr3t-value"]"#),
    );
    assert_eq!(code, Some(2), "{stderr}");
    assert!(
        stderr.contains("MCP_CONFORMANCE_CONTEXT is not a JSON object"),
        "{stderr}"
    );
    assert!(!stderr.contains("s3cr3t-value"), "{stderr}");
    assert_eq!(site.authorizations().len(), 0);
}

#[test]
fn the_flag_needs_a_url() {
    let (code, stderr) = host(&["--oauth-conformance"], None);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(
        stderr.contains("--oauth-conformance needs a server URL"),
        "{stderr}"
    );
    // A stdio server has no HTTP challenge to answer: clap refuses the pair.
    let (code, stderr) = host(&["--oauth-conformance", "--server-cmd", "server"], None);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

#[test]
fn a_url_that_is_not_one_fails_before_anything_is_sent() {
    let (code, stderr) = host(&["--oauth-conformance", "--url", "not-a-url"], None);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stderr.contains("cannot set up authorization for not-a-url"),
        "{stderr}"
    );
}
