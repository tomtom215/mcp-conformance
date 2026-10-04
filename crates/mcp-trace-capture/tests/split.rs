// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `{session}` in `-o` as a client uses it: a relaunched stdio wrapper takes the next
//! number instead of overwriting, the proxy writes a file per session, and a
//! single-file proxy says when a second session joins its trace.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::{Output, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcp-trace-capture-split-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The wrapper around `cat`, fed `input`, as a client launches it.
#[cfg(unix)]
fn launch(args: &[&str], input: &[u8]) -> Output {
    use std::io::Write as _;
    let mut child = binary()
        .args(args)
        .args(["stdio", "--", "cat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[cfg(unix)]
#[test]
fn a_relaunched_stdio_wrapper_takes_the_next_number_even_with_force() {
    let dir = scratch("relaunch");
    let template = dir.join("session-{session}.jsonl");
    let template = template.to_str().unwrap();
    let ping = |id: u32| format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"ping\"}}\n");
    // `--force`, as a client configuration carries it, does not make a numbered
    // trace overwrite the last launch's.
    for (launch_number, args) in [vec!["-o", template], vec!["-o", template, "--force"]]
        .iter()
        .enumerate()
    {
        let id = u32::try_from(launch_number).unwrap() + 1;
        let output = launch(args, ping(id).as_bytes());
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let path = dir.join(format!("session-00{id}.jsonl"));
        assert!(
            stderr.contains(&format!("recording to {}", path.display())),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("mcp-trace-validator validate {}", path.display())),
            "{stderr}"
        );
    }
    assert_eq!(files(&dir), ["session-001.jsonl", "session-002.jsonl"]);
    for id in 1..=2 {
        let text = std::fs::read_to_string(dir.join(format!("session-00{id}.jsonl"))).unwrap();
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        // open, request, echo, close — this launch's, from seq 0.
        assert_eq!(events.len(), 4, "{text}");
        assert_eq!(events[0]["seq"], 0);
        assert_eq!(events[1]["payload"]["id"], id);
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_misplaced_placeholder_or_missing_directory_is_a_usage_error() {
    let dir = scratch("misplaced");
    for (path, expected) in [
        (dir.join("{session}/trace.jsonl"), "not a directory"),
        (dir.join("{session}-{session}.jsonl"), "only once"),
        (dir.join("absent/{session}.jsonl"), "absent"),
    ] {
        for mode in [
            &["stdio", "--", "true"][..],
            &[
                "http",
                "--listen",
                "127.0.0.1:0",
                "--upstream",
                "http://127.0.0.1:9",
            ],
        ] {
            let output = binary()
                .args(["-o", path.to_str().unwrap()])
                .args(mode)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{path:?} {mode:?}: {output:?}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(expected), "{stderr}");
        }
    }
    assert!(files(&dir).is_empty(), "nothing was created");
    std::fs::remove_dir_all(&dir).ok();
}

/// clap reads `{n}` in help text as a line break; the placeholder must reach
/// `--help` as typed, or the help describes a path no one can write.
#[test]
fn help_shows_the_placeholder_as_typed() {
    let output = binary().arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("With `{session}` in the file name"), "{help}");
    assert!(
        help.contains("`traces/{session}.jsonl` writes `traces/001.jsonl`"),
        "{help}"
    );
}
