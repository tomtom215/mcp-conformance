// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask draft-capture` — record two `2026-07-28` sessions, one per
//! transport, and judge both.
//!
//! The client in each is this workspace's own `mcp-reference-host`, speaking
//! rmcp's stateless lifecycle (`server/discover`, then a `_meta` envelope per
//! request, and MRTR rounds driven by rmcp). The official runner cannot supply
//! it: the suite drives servers over `--url` only, so stdio has no runner at
//! all, and over HTTP its scenarios exercise a fixed feature set rather than
//! the surface a registry judges.
//!
//! **The two legs are recorded by different ends, and that is the point.**
//! stdio is recorded by the *host*, whose `Transport` seam carries protocol
//! messages and nothing else — redaction by construction, and no HTTP framing
//! to record even if there were any. HTTP is recorded by the *server's tap*,
//! which sits above the transport and sees status lines and headers, so its
//! recording is the only one that can bear on the twenty-four Streamable HTTP
//! clauses (`TRAN-057`…`TRAN-102`) at all. Driving the same session both ways
//! and recording it from both ends is what makes the pair complementary
//! instead of redundant.
//!
//! **That is a weaker provenance than the HTTP captures**, and the corpus
//! ledger says so where the trace is recorded rather than only here: both ends
//! of this session are ours. What it still supplies that an authored fixture
//! cannot is that neither end was written to satisfy the checks, and that
//! every byte was produced by the same rmcp machinery a third-party client
//! would use — the lifecycle, the envelope, and the MRTR retry loop are rmcp's
//! code, not ours.
//!
//! The flags below are the capture's definition, not an operator's taste, and
//! each buys clauses no other flag reaches:
//!
//! - the error budget, because sweeping every tool meets `test_error_handling`,
//!   whose whole job is to return an error result;
//! - `--subscribe`, because `subscriptions/listen` is a long-lived request
//!   rather than a tool, so no sweep of the tool list would ever reach it;
//! - `--sweep`, because the tool list is a fraction of the surface — without it
//!   the prompts, resources, templates, completion and error-code clauses have
//!   no traffic to judge and report *not observed*;
//! - `--log-level`, because `2026-07-28` requires a server to stay silent for a
//!   request that did not ask, so a recording that never asks cannot tell a
//!   conforming server from one with no logging at all.
//!
//! Like `conformance` and `draft-readiness` this is orchestration — it spawns
//! processes and speaks a real transport, which `cargo test` never does.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mcp_conformance_core::requirement::RegistrySet;
use mcp_trace_validator::report::Outcome;

/// The revision captured.
const REVISION: &str = "2026-07-28";

/// Where the run's artifacts land, kept away from `target/conformance/` and
/// `target/draft-readiness/` so no run can be mistaken for another's.
const RESULTS_DIR: &str = "target/draft-capture";

/// The committed stdio copy, relative to the workspace root.
const COMMITTED_STDIO: &str = "corpus/draft/captured/reference-host-2026-07-28-stdio.jsonl";

/// The committed Streamable HTTP copy.
const COMMITTED_HTTP: &str = "corpus/draft/captured/reference-host-2026-07-28-http.jsonl";

/// How many error *results* the tool loop tolerates.
///
/// `test_error_handling` returns one by design, and a capture that stopped
/// there would omit every tool after it alphabetically — including
/// `test_sampling`, the one that exercises an MRTR sampling round. Four is
/// slack for that one plus room to notice if the number grows.
///
/// The feature sweep's own expected failure — the read of a URI the catalog
/// does not contain — is not counted here: the sweep records every step and
/// bounds nothing, because its errors are evidence rather than a budget.
const ERROR_BUDGET: &str = "4";

/// The level every request asks for logs at.
///
/// `debug` is the floor of RFC 5424's eight, so it admits every message the
/// server might emit; a recording exists to carry what there is, not to filter
/// it. Asking is also the whole client-side half of the mechanism that
/// replaced `logging/setLevel` at this revision.
const LOG_LEVEL: &str = "debug";

/// Turn cap, above the tool count so the sweep is not silently truncated.
const TURN_LIMIT: &str = "32";

pub(crate) fn run(bless: bool) -> ExitCode {
    let root = crate::workspace_root();
    let results = root.join(RESULTS_DIR);
    if let Err(message) = prepare(&root, &results) {
        eprintln!("xtask: draft-capture — {message}");
        return ExitCode::FAILURE;
    }
    // Both legs run even when the first fails to judge clean: a defect that
    // shows on one transport and not the other is exactly what the pair
    // exists to show, and stopping at the first would hide the comparison.
    let stdio = leg(&root, &results, Leg::Stdio, bless);
    let http = leg(&root, &results, Leg::Http, bless);
    if stdio && http {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Which transport a leg records, and where its committed copy lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leg {
    /// Recorded by the host, at the `Transport` seam: messages only.
    Stdio,
    /// Recorded by the server's tap: messages plus HTTP status and headers.
    Http,
}

impl Leg {
    const fn name(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }

    const fn committed(self) -> &'static str {
        match self {
            Self::Stdio => COMMITTED_STDIO,
            Self::Http => COMMITTED_HTTP,
        }
    }
}

/// Records one leg, judges it, and refreshes its committed copy when blessing.
fn leg(root: &Path, results: &Path, leg: Leg, bless: bool) -> bool {
    let results = results.join(leg.name());
    let trace = match record(root, &results, leg) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("xtask: draft-capture — {} — {message}", leg.name());
            return false;
        }
    };
    if let Err(message) = judge(&trace, leg) {
        eprintln!("xtask: draft-capture — {} — {message}", leg.name());
        return false;
    }
    if !bless {
        eprintln!(
            "xtask: draft-capture — {} — recording at {} judges clean; BLESS=1 to \
             replace the committed copy",
            leg.name(),
            trace.display()
        );
        return true;
    }
    let committed = leg.committed();
    match std::fs::copy(&trace, root.join(committed)) {
        Ok(_) => {
            eprintln!(
                "xtask: draft-capture — {} — committed copy refreshed ({committed}); \
                 re-bless the goldens with `cargo xtask bless`",
                leg.name()
            );
            true
        }
        Err(error) => {
            eprintln!("xtask: draft-capture — cannot update {committed}: {error}");
            false
        }
    }
}

/// Builds the two binaries and clears the previous run's artifacts.
fn prepare(root: &Path, results: &Path) -> Result<(), String> {
    if let Err(error) = std::fs::remove_dir_all(results)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!("cannot clear {}: {error}", results.display()));
    }
    eprintln!("xtask: draft-capture — building the server and the host");
    let built = Command::new("cargo")
        .args([
            "build",
            "-p",
            "mcp-everything-server",
            "-p",
            "mcp-reference-host",
            "--all-features",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run cargo build: {error}"))?;
    if !built.success() {
        return Err(format!("cargo build failed with {built}"));
    }
    Ok(())
}

/// Drives one session over `leg`'s transport and returns the trace it wrote.
///
/// stdio spawns the server as a child of the host and takes the host's own
/// recording. HTTP spawns the server first, on an OS-assigned port, and takes
/// the *server's* tap instead — the host's `Transport` seam carries no headers
/// by construction, so a host-side HTTP recording would judge the transport
/// clauses on evidence it structurally cannot hold.
fn record(root: &Path, results: &Path, leg: Leg) -> Result<PathBuf, String> {
    std::fs::create_dir_all(results)
        .map_err(|error| format!("cannot create {}: {error}", results.display()))?;
    match leg {
        Leg::Stdio => record_stdio(root, results),
        Leg::Http => record_http(root, results),
    }
}

/// The freshly built binary of `name`.
fn binary(root: &Path, name: &str) -> String {
    root.join(format!(
        "target/debug/{name}{}",
        std::env::consts::EXE_SUFFIX
    ))
    .display()
    .to_string()
}

/// The host's arguments that define the capture, shared by both legs.
///
/// One list rather than two, because the whole value of the pair is that the
/// *session* is the same and only the transport differs; a flag that reached
/// one leg and not the other would make every difference between the two
/// reports ambiguous.
fn session_args() -> Vec<&'static str> {
    vec![
        "--protocol-version",
        REVISION,
        "--error-budget",
        ERROR_BUDGET,
        "--turn-limit",
        TURN_LIMIT,
        // `subscriptions/listen` is the one `2026-07-28` feature no tool call
        // reaches: it is a long-lived request, not a tool, so a sweep of the
        // tool list would record everything about this server except the
        // mechanism the revision introduced to replace `resources/subscribe`.
        "--subscribe",
        // The rest of the surface: prompts, resources, templates, completion,
        // and the one read that draws an error. Without it the recording
        // evidences the tool clauses and almost nothing else.
        "--sweep",
        "--log-level",
        LOG_LEVEL,
    ]
}

/// stdio: the host spawns the server and records at its own transport seam.
fn record_stdio(root: &Path, results: &Path) -> Result<PathBuf, String> {
    let server = format!(
        "{} --transport stdio --protocol-version {REVISION}",
        binary(root, "mcp-everything-server")
    );
    eprintln!("xtask: draft-capture — stdio — host against {server}");
    let status = Command::new(binary(root, "mcp-reference-host"))
        .args(["--server-cmd", &server])
        .args(session_args())
        .arg("--trace-dir")
        .arg(results)
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run the reference host: {error}"))?;
    if !status.success() {
        return Err(format!("the host exited {status}; no capture taken"));
    }
    sole_trace(results)
}

/// HTTP: the server is spawned first and taps its own side of the wire.
fn record_http(root: &Path, results: &Path) -> Result<PathBuf, String> {
    let (mut server, address) = crate::conformance::start_server(root, results, REVISION)
        .ok_or_else(|| "the server did not come up on HTTP".to_owned())?;
    let url = format!("http://{address}/mcp");
    eprintln!("xtask: draft-capture — http — host against {url}");
    let status = Command::new(binary(root, "mcp-reference-host"))
        .args(["--url", &url])
        .args(session_args())
        .current_dir(root)
        .status();
    // The server is killed either way: a host that failed mid-session must not
    // leave a listener holding the port for the next run.
    let _ = server.kill();
    let _ = server.wait();
    let status = status.map_err(|error| format!("cannot run the reference host: {error}"))?;
    if !status.success() {
        return Err(format!("the host exited {status}; no capture taken"));
    }
    sole_trace(results)
}

/// The one trace a leg wrote into `results`.
///
/// Named by whichever recorder produced it — the host uses scenario + pid, the
/// tap uses its own sequence — so this reads the directory rather than
/// duplicating either convention, which is what keeps concurrent runs from
/// colliding.
fn sole_trace(results: &Path) -> Result<PathBuf, String> {
    let mut traces: Vec<PathBuf> = std::fs::read_dir(results)
        .map_err(|error| format!("cannot read {}: {error}", results.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    traces.sort();
    traces
        .pop()
        .ok_or_else(|| format!("nothing was recorded into {}", results.display()))
}

/// Fails unless every judged clause of the revision passes.
///
/// A *capture* normally asserts no verdict — a real implementation is whatever
/// it is, and `corpus/draft/captured/` pins reports rather than outcomes. This
/// one is different because both ends are this workspace's: the recording is
/// evidence about our own server, so a finding in it is a defect to fix rather
/// than news about somebody else's code.
fn judge(trace: &Path, leg: Leg) -> Result<(), String> {
    let document = std::fs::read_to_string(trace)
        .map_err(|error| format!("cannot read {}: {error}", trace.display()))?;
    let events = mcp_trace_validator::reader::parse_trace(
        &document,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .map_err(|error| format!("{} is malformed: {error}", trace.display()))?;
    let set = RegistrySet::builtin().map_err(|error| format!("registry set: {error}"))?;
    let revision = REVISION
        .parse()
        .map_err(|_| format!("{REVISION} is not a protocol revision"))?;
    let registry = set.registry(revision).ok_or_else(|| {
        format!("this build does not describe {REVISION}; enable `draft-2026-07-28`")
    })?;
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    let failed: Vec<&str> = report
        .requirements
        .iter()
        .filter(|row| matches!(row.outcome, Outcome::Fail | Outcome::Warn))
        .map(|row| row.id.as_str())
        .collect();
    let counts = report.totals;
    // `not observed` is named because it is the honest denominator: a capture
    // that passes 77 of the 124 judgeable clauses has evidenced 77, and the
    // number is the one to watch when the session is enriched.
    eprintln!(
        "xtask: draft-capture — {} — {} pass, {} fail, {} warn, {} not observed, {} excluded",
        leg.name(),
        counts.pass,
        counts.fail,
        counts.warn,
        counts.not_observed,
        counts.excluded
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the capture is not clean: {}. Both ends of this session are ours, so a \
             finding here is a defect in this workspace, not news about another \
             implementation.",
            failed.join(", ")
        ))
    }
}
