// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The exerciser leg of `cargo xtask conformance` (also `cargo xtask
//! exerciser` on its own, which needs no Node.js): `mcp-conformance check`
//! against the everything server over stdio and Streamable HTTP, at both
//! revisions, calling every tool (ADR-0024, decision 10).
//!
//! Two things are held, per session:
//!
//! 1. **`check` exits 0**: the server breaks no MUST-level clause, as the
//!    suite's own verdict on it says it should not.
//! 2. **The recording judges clean under `validate --strict` with no party
//!    view.** `check`'s verdict counts the server's findings; this counts the
//!    exerciser's too, and warnings with them. The exerciser is a client
//!    written by hand, and the validator is the independent reading of the
//!    specification it is held to: a slip in the requests it builds, or in
//!    the headers it mirrors, fails here.
//!
//! And one thing across them: each session ran under the revision it was
//! meant to, so a silent fall back to the other era cannot pass for coverage.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use super::start_server;

/// Where this leg writes, under the workspace root.
const DIR: &str = "target/conformance/exerciser";

/// The revisions the everything server serves, each exercised on both
/// transports.
const REVISIONS: [&str; 2] = ["2025-11-25", "2026-07-28"];

pub(crate) fn run(root: &Path) -> ExitCode {
    match legs(root) {
        Ok(sessions) => {
            eprintln!(
                "xtask: conformance — exerciser leg green: `mcp-conformance check` judged the \
                 everything server clean in {sessions} sessions (stdio and HTTP at {}), and \
                 broke no clause itself",
                REVISIONS.join(" and ")
            );
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("xtask: exerciser — {message}");
            ExitCode::FAILURE
        }
    }
}

fn legs(root: &Path) -> Result<usize, String> {
    eprintln!("xtask: conformance — building mcp-conformance and the everything server");
    let build = Command::new("cargo")
        .args([
            "build",
            "-p",
            "mcp-conformance-cli",
            "-p",
            "mcp-everything-server",
        ])
        // As `conformance` builds them, so the two share one build.
        .arg("--all-features")
        .current_dir(root)
        .status();
    if !matches!(build, Ok(status) if status.success()) {
        return Err("the build failed".to_owned());
    }
    let dir = root.join(DIR);
    if let Err(error) = std::fs::remove_dir_all(&dir)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!("cannot clear {}: {error}", dir.display()));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let server = binary(root, "mcp-everything-server");
    let mut sessions = 0;
    for revision in REVISIONS {
        // The legacy mode is rmcp's dual-era surface: asked `server/discover`
        // it names `2026-07-28` too, so the legacy sessions name the revision
        // they mean, and the modern ones take the default (the probe).
        let speak = if revision == REVISIONS[0] {
            vec!["--spec-version".to_owned(), revision.to_owned()]
        } else {
            Vec::new()
        };
        let stdio = [
            speak.clone(),
            vec![
                "--".to_owned(),
                server.display().to_string(),
                "--protocol-version".to_owned(),
                revision.to_owned(),
            ],
        ]
        .concat();
        session(root, &dir, &format!("stdio-{revision}"), revision, &stdio)?;
        let Some((mut child, address)) = start_server(root, &dir.join("tap"), revision) else {
            return Err("the everything server did not start".to_owned());
        };
        let http = [
            speak,
            vec!["--url".to_owned(), format!("http://{address}/mcp")],
        ]
        .concat();
        let outcome = session(root, &dir, &format!("http-{revision}"), revision, &http);
        let _ = child.kill();
        let _ = child.wait();
        outcome?;
        sessions += 2;
    }
    Ok(sessions)
}

/// One `check --call-all` run against `subject`, then its two assertions.
fn session(
    root: &Path,
    dir: &Path,
    name: &str,
    revision: &str,
    subject: &[String],
) -> Result<(), String> {
    let trace = dir.join(format!("{name}.jsonl"));
    let summary = dir.join(format!("{name}.summary.json"));
    eprintln!("xtask: exerciser — mcp-conformance check ({name})");
    let checked = Command::new(binary(root, "mcp-conformance"))
        .args(["check", "--call-all", "-o"])
        .arg(&trace)
        .arg("--summary")
        .arg(&summary)
        .args(subject)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|error| format!("cannot run mcp-conformance: {error}"))?;
    if !checked.success() {
        return Err(format!(
            "`mcp-conformance check` ({name}) did not judge the everything server clean \
             ({checked}); re-run it on {} for the report",
            trace.display()
        ));
    }
    let ran = std::fs::read_to_string(&summary)
        .map_err(|error| format!("cannot read {}: {error}", summary.display()))?;
    let ran: serde_json::Value =
        serde_json::from_str(&ran).map_err(|error| format!("{}: {error}", summary.display()))?;
    if ran.get("revision").and_then(serde_json::Value::as_str) != Some(revision) {
        return Err(format!(
            "{name}: the session ran under {} where {revision} was served",
            ran.get("revision").unwrap_or(&serde_json::Value::Null)
        ));
    }
    let exercised = |kind: &str| {
        ran.pointer(&format!("/{kind}/exercised"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    if exercised("tools") == 0 || exercised("resources") == 0 || exercised("prompts") == 0 {
        return Err(format!(
            "{name}: a tool, a resource and a prompt were each to be exercised; see {}",
            summary.display()
        ));
    }
    let both = Command::new(binary(root, "mcp-conformance"))
        .args(["validate", "--strict"])
        .arg(&trace)
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run mcp-conformance: {error}"))?;
    if !both.status.success() {
        return Err(format!(
            "{name}: judged for both parties and strictly, the recording is not clean — a \
             finding held against the exerciser is a defect in `check`:\n{}",
            String::from_utf8_lossy(&both.stdout)
        ));
    }
    Ok(())
}

fn binary(root: &Path, name: &str) -> PathBuf {
    root.join(format!(
        "target/debug/{name}{}",
        std::env::consts::EXE_SUFFIX
    ))
}
