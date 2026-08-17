// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The run record: what each phase of a session did, on stderr.
//!
//! Split from `main` so the binary's own file stays about *dispatch* — which
//! transport, which lifecycle, which phases — rather than about formatting.
//! Everything here writes to stderr: stdout stays silent, because the official
//! suite captures it and a future stdout report format must not have to fight
//! old noise.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, exactly as `xtask` does.
#![allow(clippy::redundant_pub_crate)]

use std::process::ExitCode;

use mcp_reference_host::run::RunReport;
use mcp_reference_host::sweep::SweepReport;

/// Sends the probe session and reports what each malformed request drew.
///
/// Exits clean whatever the answers are. The probe's product is the
/// *recording*, judged afterwards by the registry against the server's tap; a
/// host that decided for itself which answer was right would be a second,
/// weaker implementation of the checks, and the two would drift.
#[cfg(feature = "http")]
pub(crate) async fn probe(url: &str) -> ExitCode {
    let outcomes = mcp_reference_host::probe::run(url).await;
    eprintln!(
        "mcp-reference-host: probed {} malformed request(s)",
        outcomes.len()
    );
    for outcome in &outcomes {
        match &outcome.answer {
            Ok(status) => {
                eprintln!("  HTTP {status:<3} [{}] {}", outcome.clauses, outcome.fault);
            }
            Err(error) => eprintln!(
                "  ----     [{}] {}: {error}",
                outcome.clauses, outcome.fault
            ),
        }
    }
    ExitCode::SUCCESS
}

/// The sweep record, one line per step, on stderr.
///
/// Errors are printed but not counted against the exit code: the sweep ends
/// with a read that is *meant* to fail, and a host that exited non-zero for it
/// would make the capture harness fail on its own design.
pub(crate) fn sweep(report: &SweepReport) {
    eprintln!(
        "mcp-reference-host: swept {} step(s), {} drew errors",
        report.steps.len(),
        report.errors()
    );
    for step in &report.steps {
        match &step.outcome {
            Ok(summary) => eprintln!("  ok   {}: {summary}", step.what),
            Err(error) => eprintln!("  err  {}: {error}", step.what),
        }
    }
}

/// The run record, one line per call, on stderr.
pub(crate) fn run(report: &RunReport) {
    eprintln!(
        "mcp-reference-host: {:?} after {} turn(s), {} error(s)",
        report.stop, report.turns, report.errors
    );
    for outcome in &report.outcomes {
        match &outcome.result {
            Ok(text) => eprintln!("  ok   {}: {text}", outcome.tool),
            Err(error) => eprintln!("  err  {}: {error}", outcome.tool),
        }
    }
}
