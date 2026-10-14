// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The real binary: its arguments, its exit codes, and — with a stand-in for
//! Node and the suite — a whole `test --command` run, offline.
//!
//! No test here reaches a network or the real suite: that is `cargo xtask
//! conformance`'s product leg, the one place this repository runs npx. The
//! stand-ins are `sh` scripts (Unix only, as the recorder's own process tests
//! are), and the stand-in suite drives the bridge with `curl` over loopback.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-conformance"))
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("mcp-conformance-cli-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_names_the_subcommand_and_the_exit_codes() {
    let output = binary().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("test"), "{help}");
    assert!(
        !help.contains("__client-relay"),
        "the relay is not offered: {help}"
    );
    let output = binary().args(["test", "--help"]).output().unwrap();
    let help = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "--url",
        "--command",
        "--client-command",
        "Exit status:",
        "Node.js 18",
    ] {
        assert!(help.contains(needle), "{needle}: {help}");
    }
}

/// `watch` takes `mcp-trace-capture`'s flags, by the same names, and its exit
/// codes are the recorder's.
#[test]
fn watch_help_names_the_recorders_modes_and_flags() {
    let output = binary().args(["watch", "--help"]).output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "stdio",
        "http",
        "--output",
        "--force",
        "--max-message-bytes",
        "--otlp",
        "mcp-trace-capture's",
    ] {
        assert!(help.contains(needle), "{needle}: {help}");
    }
    let output = binary().args(["watch", "http", "--help"]).output().unwrap();
    let help = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "--upstream",
        "--listen",
        "--preserve-host",
        "127.0.0.1:8080",
    ] {
        assert!(help.contains(needle), "{needle}: {help}");
    }
    let empty = scratch("watch-bad-otlp");
    let bad = binary()
        .args([
            "watch",
            "--otlp",
            "http://u:s3cret@c/",
            "stdio",
            "--",
            "true",
        ])
        .current_dir(&empty)
        .output()
        .unwrap();
    assert_eq!(
        std::fs::read_dir(&empty).unwrap().count(),
        0,
        "refused before any trace is created"
    );
    std::fs::remove_dir_all(&empty).ok();
    assert_eq!(bad.status.code(), Some(2));
    assert!(!stderr(&bad).contains("s3cret"), "{}", stderr(&bad));
    assert!(
        stderr(&bad).contains("--otlp: the URL carries credentials"),
        "{}",
        stderr(&bad)
    );
}

#[test]
fn version_is_the_crate_version() {
    let output = binary().arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("mcp-conformance {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn exactly_one_subject_is_required() {
    let none = binary().arg("test").output().unwrap();
    assert_eq!(none.status.code(), Some(2), "{}", stderr(&none));
    let two = binary()
        .args([
            "test",
            "--url",
            "http://localhost:1/mcp",
            "--command",
            "server",
        ])
        .output()
        .unwrap();
    assert_eq!(two.status.code(), Some(2), "{}", stderr(&two));
    assert!(
        stderr(&two).contains("cannot be used with"),
        "{}",
        stderr(&two)
    );
}

/// Without Node on PATH, `test` says what to install and installs nothing.
#[test]
fn without_node_it_names_what_to_install() {
    let empty = scratch("no-node");
    let output = binary()
        .args(["test", "--url", "http://localhost:1/mcp"])
        .env("PATH", &empty)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let message = stderr(&output);
    assert!(message.contains("Node.js 18 or newer"), "{message}");
    assert!(message.contains("Nothing was installed."), "{message}");
    assert_eq!(
        std::fs::read_dir(&empty).unwrap().count(),
        0,
        "nothing written"
    );
    std::fs::remove_dir_all(&empty).ok();
}

#[test]
fn the_relay_refuses_to_run_outside_test() {
    let output = binary()
        .args(["__client-relay", "client", "http://localhost:1/mcp"])
        .env_remove("MCP_CONFORMANCE_RELAY_DIR")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("MCP_CONFORMANCE_RELAY_DIR"),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[path = "cli/offline.rs"]
mod offline;

#[cfg(unix)]
#[path = "cli/watch.rs"]
mod watch;

/// Splits a documented command line as a shell would for the simple cases the
/// documentation uses: whitespace, and double or single quotes.
pub(crate) fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (Some(open), c) if c == open => quote = None,
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// Every `mcp-conformance …` line in `text`'s fenced `text` blocks but the
/// `watch` ones, which start a session rather than ask for Node; the binary's
/// own `every_documented_watch_command_parses` holds those to the parser.
pub(crate) fn documented_commands(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        if line.starts_with("```") {
            in_block = line == "```text" && !in_block;
            continue;
        }
        if in_block
            && line.starts_with("mcp-conformance ")
            && !line.starts_with("mcp-conformance watch")
        {
            commands.push(line.to_owned());
        }
    }
    commands
}

/// Each documented command is one this binary accepts. A `test` command, run
/// with no Node on PATH, gets as far as asking for Node — past argument
/// parsing — rather than failing as a usage error. Any other (`serve`, which
/// would start serving; `library`, which would change a library) is run with
/// `--help` after its arguments, which clap answers only once every argument
/// before it has parsed.
#[test]
fn every_documented_command_parses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let empty = scratch("documented");
    let mut seen = 0;
    for document in [
        "README.md",
        "../../book/src/testing.md",
        "../../book/src/workbench.md",
    ] {
        let text = std::fs::read_to_string(root.join(document)).unwrap();
        let commands = documented_commands(&text);
        assert!(!commands.is_empty(), "{document} shows no command");
        for command in commands {
            let mut words = words(&command);
            if words.get(1).is_some_and(|sub| sub != "test") {
                words.push("--help".to_owned());
            }
            let output = binary()
                .args(&words[1..])
                .env("PATH", &empty)
                .current_dir(&empty)
                .output()
                .unwrap();
            let message = stderr(&output);
            let help = words
                .iter()
                .any(|word| word == "--help" || word == "--version");
            assert!(
                (help && output.status.success()) || message.contains("Node.js 18 or newer"),
                "{document}: `{command}` did not parse:\n{message}"
            );
            seen += 1;
        }
    }
    assert!(seen >= 4, "{seen}");
    std::fs::remove_dir_all(&empty).ok();
}

#[test]
fn words_split_as_a_shell_would_here() {
    assert_eq!(
        words(r#"mcp-conformance test --command "python my_server.py" -o 'a b.json'"#),
        [
            "mcp-conformance",
            "test",
            "--command",
            "python my_server.py",
            "-o",
            "a b.json"
        ]
    );
    assert_eq!(words("  "), Vec::<String>::new());
    assert_eq!(words(r#"x """#), ["x", ""]);
}

#[path = "cli/library.rs"]
mod library;
