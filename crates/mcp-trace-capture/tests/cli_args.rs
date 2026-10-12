// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the binary says about the arguments it refuses or doubts, and what its
//! `--help` promises.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-args-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

fn http(trace: &std::path::Path, upstream: &str, listen: &str) -> Output {
    binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "http",
            "--upstream",
            upstream,
        ])
        .args(["--listen", listen])
        .output()
        .unwrap()
}

/// An `--upstream` that will not parse is refused without being repeated: a
/// URL can carry a password or a key, and clap's own error echoed it.
#[test]
fn an_upstream_that_will_not_parse_is_not_echoed() {
    let trace = scratch("echo.jsonl");
    for (upstream, secret) in [
        ("http://user:pa ss@host/", "pa ss"),
        ("http://h/?key=SEC RET", "SEC RET"),
        ("ftp://h/?key=SECRET", "SECRET"),
    ] {
        let output = http(&trace, upstream, "127.0.0.1:0");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains(secret), "{stderr}");
        assert!(stderr.contains("--upstream"), "{stderr}");
        assert!(!trace.exists());
    }
}

/// The near misses get the form that would have worked.
#[test]
fn near_misses_are_answered_with_the_expected_form() {
    let trace = scratch("hints.jsonl");
    let output = http(&trace, "localhost:3999", "127.0.0.1:0");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("did you mean http://localhost:3999?"),
        "{output:?}"
    );
    for (listen, hint) in [
        ("18081", "e.g. 127.0.0.1:18081"),
        ("localhost:18081", "not a host name, e.g. 127.0.0.1:18081"),
    ] {
        let output = http(&trace, "http://127.0.0.1:9/", listen);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(hint),
            "{listen}: {output:?}"
        );
    }
    assert!(!trace.exists());
}

/// A `{…}` in the file name that is not `{session}` — a typo — would silently
/// name one literal file; the capture says so.
#[cfg(unix)]
#[test]
fn a_misspelt_session_placeholder_is_warned_about() {
    let dir = scratch("typo");
    std::fs::create_dir_all(&dir).unwrap();
    let output = binary()
        .args(["-o", dir.join("{sesion}.jsonl").to_str().unwrap()])
        .args(["stdio", "--", "true"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning: {sesion}.jsonl has a {…} that is not {session}"),
        "{stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// `--help` carries the exit codes, for the top level and each subcommand.
#[test]
fn help_lists_the_exit_codes() {
    for args in [&["--help"][..], &["stdio", "--help"], &["http", "--help"]] {
        let output = binary().args(args).output().unwrap();
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("Exit status:"), "{args:?}: {help}");
        assert!(
            help.contains("1 if waiting for the server failed"),
            "{help}"
        );
        assert!(help.contains("2 if serving failed"), "{help}");
    }
}
