// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The human report stays readable when a trace repeats one defect.
//!
//! A recording that reused one request id 250,000 times rendered four million
//! lines, 425 MB, of the same finding. The default report now lists the first
//! [`LIMIT`] findings under a clause and counts the rest; `--all` and every
//! machine format still carry each one.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;
use std::process::{Command, Output};

/// The CLI's per-clause limit, as its help states it.
const LIMIT: usize = 20;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// A `2025-11-25` session whose client reuses request id 1 `reuses` times,
/// each a `BASE-003` finding.
fn reusing_trace(reuses: usize) -> String {
    let mut trace = String::from(concat!(
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#,
        "\n",
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#,
        "\n",
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
        "\n",
    ));
    for reuse in 0..reuses {
        let seq = 3 + 2 * reuse;
        let _ = writeln!(
            trace,
            r#"{{"seq":{seq},"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"method":"ping"}}}}"#
        );
        let _ = writeln!(
            trace,
            r#"{{"seq":{},"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"result":{{}}}}}}"#,
            seq + 1
        );
    }
    trace
}

#[test]
fn a_repeated_defect_is_listed_up_to_the_limit_and_counted_after_it() {
    let path = std::env::temp_dir().join(format!("mcp-tv-human-{}.jsonl", std::process::id()));
    std::fs::write(&path, reusing_trace(LIMIT + 5)).unwrap();
    let trace = path.to_str().unwrap();

    let default = run(&["validate", trace]);
    let all = run(&["validate", "--all", trace]);
    let json = run(&["validate", "--format", "json", trace]);
    std::fs::remove_file(&path).ok();

    assert_eq!(default.status.code(), Some(1), "{default:?}");
    let text = stdout(&default);
    assert_eq!(text.matches("reuses id 1").count(), LIMIT, "{text}");
    assert!(
        text.contains(
            "        … and 5 more (pass --all, or use --format json, for every finding)\n"
        ),
        "{text}"
    );

    let text = stdout(&all);
    assert_eq!(text.matches("reuses id 1").count(), LIMIT + 5, "{text}");
    assert!(!text.contains("more (pass --all"), "{text}");

    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    let base_003 = report["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "BASE-003")
        .unwrap();
    assert_eq!(
        base_003["findings"].as_array().map(Vec::len),
        Some(LIMIT + 5)
    );
}
