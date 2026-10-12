// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Flags that said less, or did less, than they should: `--strict` over
//! several traces, a repeated `--revision`, a zero limit, and `--version`.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn corpus(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
        .to_str()
        .unwrap()
        .to_owned()
}

const NOTE: &str = "note: --strict — SHOULD-level findings are treated as failures, so this \
                    run exits 1 despite a verdict of pass-with-warnings";

#[test]
fn strict_over_several_traces_notes_once_and_names_them() {
    let warning = corpus("violations/life-004-client-request-before-init-response.jsonl");
    let good = corpus("good/stdio-minimal-init.jsonl");
    let several = run(&["validate", "--strict", &warning, &good, &warning]);
    assert_eq!(several.status.code(), Some(1), "{several:?}");
    assert_eq!(
        stderr(&several),
        format!("{NOTE} for {warning}, {warning}\n"),
        "one note, naming the traces it failed and no other"
    );

    // One trace is the note as it always read.
    let alone = run(&["validate", "--strict", &warning]);
    assert_eq!(stderr(&alone), format!("{NOTE}\n"));

    // Several traces `--strict` did not promote: nothing to say.
    let clean = run(&["validate", "--strict", &good, &good]);
    assert_eq!(clean.status.code(), Some(0), "{clean:?}");
    assert_eq!(stderr(&clean), "");
}

#[test]
fn a_revision_named_twice_is_judged_once() {
    let trace = corpus("good/stdio-minimal-init.jsonl");
    let twice = run(&[
        "validate",
        "--revision",
        "2025-11-25",
        "--revision",
        "2025-11-25",
        &trace,
    ]);
    assert_eq!(twice.status.code(), Some(0), "{twice:?}");
    let once = run(&["validate", "--revision", "2025-11-25", &trace]);
    assert_eq!(stdout(&twice), stdout(&once));
    assert!(
        stdout(&twice).starts_with("MCP trace validation — revision 2025-11-25 (requested)\n"),
        "{twice:?}"
    );

    // Two distinct revisions, one repeated: two columns, in the order first named.
    let mixed = run(&[
        "validate",
        "--revision",
        "2026-07-28",
        "--revision",
        "2025-11-25",
        "--revision",
        "2026-07-28",
        &trace,
    ]);
    assert!(
        stdout(&mixed)
            .starts_with("MCP multi-revision validation — revisions 2026-07-28, 2025-11-25 "),
        "{mixed:?}"
    );
}

#[test]
fn a_limit_of_zero_is_refused_as_a_bad_flag() {
    let trace = corpus("good/stdio-minimal-init.jsonl");
    for flag in ["--max-events", "--max-line-bytes"] {
        let zero = run(&["validate", flag, "0", &trace]);
        assert_eq!(zero.status.code(), Some(2), "{zero:?}");
        assert!(
            stderr(&zero).contains("must be at least 1: a limit of 0 accepts no trace at all"),
            "{zero:?}"
        );
        // One is a limit like any other: the trace is over it, not the flag wrong.
        let one = run(&["validate", flag, "1", &trace]);
        assert_eq!(one.status.code(), Some(3), "{one:?}");
        let word = run(&["validate", flag, "many", &trace]);
        assert_eq!(word.status.code(), Some(2), "{word:?}");
        assert!(stderr(&word).contains("invalid digit"), "{word:?}");
    }
}

#[test]
fn version_keeps_its_first_line_and_names_the_revisions_judged() {
    let short = stdout(&run(&["-V"]));
    assert_eq!(
        short,
        format!("mcp-trace-validator {}\n", env!("CARGO_PKG_VERSION"))
    );
    let long = stdout(&run(&["--version"]));
    assert_eq!(
        long,
        format!(
            "mcp-trace-validator {}\njudges protocol revisions: 2025-11-25, 2026-07-28\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}
