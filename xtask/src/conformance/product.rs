// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The product leg of `cargo xtask conformance`: `mcp-conformance test`
//! (ADR-0021, roadmap W1) against the same subjects the legs before it gated,
//! and its report held to theirs.
//!
//! Three runs of the real binary, each its JSON report:
//!
//! 1. **`--url`** — the everything server over streamable HTTP, through the
//!    recorder's proxy;
//! 2. **`--command`** — the everything server over stdio, through the stdio
//!    bridge (the suite drives servers only over HTTP);
//! 3. **`--client-command`** — the reference host as the suite's client.
//!
//! "Agrees with `cargo xtask conformance` on the same subject" is checked, not
//! eyeballed (`compare.rs`):
//!
//! - **the suite's side is identical**: every scenario the gate's run reported,
//!   with the same check statuses (for stdio: the gate's server scenarios less
//!   the HTTP-transport ones, which `test` does not run against a stdio server;
//!   and the pending list `test` uses is the one the suite's default run
//!   actually leaves out);
//! - **the validator's side contains the gate's**: every requirement the gate's
//!   recordings fail, `test`'s fail too; and what `test` fails beyond them is
//!   exactly the committed `conformance/wire-findings.json` — each an exchange the gate's
//!   recorders cannot see (the server tap sits behind the `Host` check; the
//!   host's capture sits above the HTTP layer), with why. A new one fails this
//!   leg, and so does an entry that no longer occurs.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mcp_conformance_cli::suite;

use super::{EXPECTED_FAILURES, start_server};

mod compare;

use compare::{Leg, Report};

/// Where this leg writes, under the workspace root.
const PRODUCT_DIR: &str = "target/conformance/product";

/// The committed findings only a wire-level recording shows, relative to the
/// workspace root.
pub(crate) const WIRE_FINDINGS: &str = "conformance/wire-findings.json";

/// The server divergence baseline the gate uses, passed to `test` too.
const SERVER_BASELINE: &str = "conformance/agreement-divergences.json";

/// The client divergence baseline the gate uses, passed to `test` too.
const CLIENT_BASELINE: &str = "conformance/client-agreement-divergences.json";

pub(crate) fn run(root: &Path) -> ExitCode {
    match legs(root) {
        Ok(()) => {
            eprintln!(
                "xtask: conformance — product leg green: `mcp-conformance test` agrees with \
                 this gate on the everything server (HTTP and stdio) and the reference host"
            );
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("xtask: product — {message}");
            ExitCode::FAILURE
        }
    }
}

fn legs(root: &Path) -> Result<(), String> {
    eprintln!("xtask: conformance — building mcp-conformance");
    let build = Command::new("cargo")
        .args(["build", "-p", "mcp-conformance-cli"])
        .current_dir(root)
        .status();
    if !matches!(build, Ok(status) if status.success()) {
        return Err("the mcp-conformance build failed".to_owned());
    }
    let dir = root.join(PRODUCT_DIR);
    if let Err(error) = std::fs::remove_dir_all(&dir)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!("cannot clear {}: {error}", dir.display()));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let wire = compare::WireFindings::load(&root.join(WIRE_FINDINGS))?;
    let gate_server = compare::Gate::read(
        &root.join("target/conformance"),
        &root.join("target/conformance/agreement.json"),
    )?;
    let gate_client = compare::Gate::read(
        &root.join("target/conformance/client"),
        &root.join("target/conformance/client/agreement.json"),
    )?;

    let http = http_leg(root, &dir)?;
    compare::check(Leg::Http, &gate_server, &http, &wire)?;
    let stdio = test(
        root,
        &dir,
        "stdio",
        &[
            "--command".to_owned(),
            format!(
                "{} --transport stdio",
                binary(root, "mcp-everything-server").display()
            ),
        ],
    )?;
    compare::check(Leg::Stdio, &gate_server, &stdio, &wire)?;
    compare::pending_is_what_the_default_run_leaves_out(&gate_server, &stdio)?;
    let client = test(
        root,
        &dir,
        "client",
        &[
            "--client-command".to_owned(),
            binary(root, "mcp-reference-host").display().to_string(),
            "--divergences".to_owned(),
            root.join(CLIENT_BASELINE).display().to_string(),
        ],
    )?;
    compare::check(Leg::Client, &gate_client, &client, &wire)?;
    wire.all_observed()
}

/// `test --url` against a freshly started everything server.
fn http_leg(root: &Path, dir: &Path) -> Result<Report, String> {
    let Some((mut server, address)) = start_server(root, &dir.join("tap"), suite::SPEC_VERSION)
    else {
        return Err("the everything server did not start".to_owned());
    };
    let report = test(
        root,
        dir,
        "http",
        &[
            "--url".to_owned(),
            format!("http://{address}/mcp"),
            "--divergences".to_owned(),
            root.join(SERVER_BASELINE).display().to_string(),
        ],
    );
    let _ = server.kill();
    let _ = server.wait();
    report
}

/// Runs `mcp-conformance test` with `subject` and reads its JSON report. Exit
/// 0 and 1 are verdicts, compared afterwards; 2 is a run that failed.
fn test(root: &Path, dir: &Path, name: &str, subject: &[String]) -> Result<Report, String> {
    let work = dir.join(name);
    let report = dir.join(format!("{name}.json"));
    eprintln!("xtask: product — mcp-conformance test ({name})");
    let status = Command::new(binary(root, "mcp-conformance"))
        .arg("test")
        .args(subject)
        .arg("--expected-failures")
        .arg(root.join(EXPECTED_FAILURES))
        .arg("--work-dir")
        .arg(&work)
        .args(["--format", "json", "-o"])
        .arg(&report)
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run mcp-conformance: {error}"))?;
    match status.code() {
        Some(0 | 1) => Report::read(&report),
        _ => Err(format!(
            "`mcp-conformance test` ({name}) could not make its run ({status}); see its \
             stderr above"
        )),
    }
}

fn binary(root: &Path, name: &str) -> PathBuf {
    root.join(format!(
        "target/debug/{name}{}",
        std::env::consts::EXE_SUFFIX
    ))
}
