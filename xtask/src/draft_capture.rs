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

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, ExitCode};

use mcp_conformance_core::requirement::RegistrySet;
use mcp_trace_validator::report::Outcome;

mod record;

use record::record;

/// The revision captured.
const REVISION: &str = "2026-07-28";

/// Where the run's artifacts land, kept away from `target/conformance/` and
/// `target/draft-readiness/` so no run can be mistaken for another's.
const RESULTS_DIR: &str = "target/draft-capture";

/// The committed stdio copy, relative to the workspace root.
const COMMITTED_STDIO: &str = "corpus/draft/captured/reference-host-2026-07-28-stdio.jsonl";

/// The committed Streamable HTTP copy.
const COMMITTED_HTTP: &str = "corpus/draft/captured/reference-host-2026-07-28-http.jsonl";

/// The committed probe session.
const COMMITTED_PROBE: &str = "corpus/draft/captured/probe-2026-07-28-http.jsonl";

/// The ledger of server-side findings the probe is *expected* to draw.
///
/// A conforming client cannot exercise a rejection rule, so the probe is not
/// one — and its recording therefore cannot be held to "clean" the way the
/// other two legs are. What it is held to is this file: every server-side
/// finding must be listed here with a reason, and every listed finding must
/// still occur. A defect that is known, dated and explained is honest; one
/// that quietly stops being reported is a check that stopped working.
const PROBE_BASELINE: &str = "conformance/probe-baseline.json";

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
    let probe = leg(&root, &results, Leg::Probe, bless);
    if stdio && http && probe {
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
    /// The deliberately malformed session, also tapped by the server. Judged
    /// against [`PROBE_BASELINE`] rather than for cleanliness.
    Probe,
}

impl Leg {
    const fn name(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::Probe => "probe",
        }
    }

    const fn committed(self) -> &'static str {
        match self {
            Self::Stdio => COMMITTED_STDIO,
            Self::Http => COMMITTED_HTTP,
            Self::Probe => COMMITTED_PROBE,
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
    if leg == Leg::Probe {
        return reconcile(&failed);
    }
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

/// One entry of [`PROBE_BASELINE`]: a finding the probe is expected to draw.
#[derive(Debug, serde::Deserialize)]
struct Expected {
    /// The requirement it fires against.
    requirement: String,
    /// Why it is expected — the client's own fault, or an open server defect.
    #[allow(dead_code, reason = "read by humans; the gate compares ids only")]
    why: String,
}

/// The committed ledger of what the probe is expected to draw.
#[derive(Debug, serde::Deserialize)]
struct ProbeBaseline {
    expected: Vec<Expected>,
}

/// Holds the probe's findings to [`PROBE_BASELINE`], both directions.
///
/// A probe is not a conforming client, so "clean" is the wrong bar — the
/// client-side clauses it breaks are the faults under test. What is gated is
/// that the set has not *moved*: an unexpected finding is a regression or a new
/// defect, and an expected one that stopped occurring is either a fix that
/// should retire its entry or a check that quietly stopped working. Both fail,
/// and neither is blessed away — every entry carries a hand-written reason,
/// which a generated file could not.
fn reconcile(failed: &[&str]) -> Result<(), String> {
    let path = crate::workspace_root().join(PROBE_BASELINE);
    let document = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let baseline: ProbeBaseline = serde_json::from_str(&document)
        .map_err(|error| format!("{} is malformed: {error}", path.display()))?;
    let expected: BTreeSet<&str> = baseline
        .expected
        .iter()
        .map(|entry| entry.requirement.as_str())
        .collect();
    let observed: BTreeSet<&str> = failed.iter().copied().collect();
    let unexpected: Vec<&&str> = observed.difference(&expected).collect();
    let stale: Vec<&&str> = expected.difference(&observed).collect();
    if unexpected.is_empty() && stale.is_empty() {
        eprintln!(
            "xtask: draft-capture — probe — {} expected finding(s), reconciled against {PROBE_BASELINE}",
            expected.len()
        );
        return Ok(());
    }
    let mut sections = Vec::new();
    if !unexpected.is_empty() {
        sections.push(format!(
            "{unexpected:?} are not in {PROBE_BASELINE}: either the probe found a new \
             defect, or a fix regressed. Add an entry with a reason, or fix it"
        ));
    }
    if !stale.is_empty() {
        sections.push(format!(
            "{stale:?} are listed in {PROBE_BASELINE} but no longer occur: retire the \
             entry in the change that fixed them, or find out why the check stopped firing"
        ));
    }
    Err(sections.join("; "))
}
