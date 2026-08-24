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

use mcp_reference_host::run::{RunPlan, RunReport, StopReason};
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

/// The cancellation round: what was cancelled, and what the server was still
/// allowed to answer afterwards.
pub(crate) fn cancel(outcome: &Result<mcp_reference_host::cancel::CancelReport, String>) {
    match outcome {
        Ok(report) => {
            eprintln!("mcp-reference-host: cancelled {}", report.cancelled);
            match &report.after {
                Ok(summary) => eprintln!("  ok   the call after it: {summary}"),
                Err(error) => eprintln!("  err  the call after it: {error}"),
            }
        }
        Err(error) => eprintln!("mcp-reference-host: cancellation round failed: {error}"),
    }
}

/// The run record, one line per call, on stderr.
pub(crate) fn run(report: &RunReport, plan: &RunPlan) {
    eprintln!("mcp-reference-host: {}", stopped(report, plan));
    for outcome in &report.outcomes {
        match &outcome.result {
            Ok(text) => eprintln!("  ok   {}: {text}", outcome.tool),
            Err(error) => eprintln!("  err  {}: {error}", outcome.tool),
        }
    }
}

/// Why the loop ended, as a sentence naming the flag that changes it.
///
/// This used to print the `StopReason` variant with `{:?}`, which named the
/// condition in Rust's words and the remedy in nobody's. The default plan
/// tolerates no errors, so a `--sweep` over the everything server — whose tool
/// list deliberately contains `test_error_handling` — always ends
/// `ErrorBudgetExhausted` and exits 1. That outcome is correct; a reader who
/// has to find `--error-budget` in `--help` to learn why is not.
///
/// Matched exhaustively on purpose (a same-crate enum): a new [`StopReason`]
/// must force a deliberate sentence here rather than fall into a wildcard that
/// prints the variant name again.
fn stopped(report: &RunReport, plan: &RunPlan) -> String {
    let RunReport {
        turns,
        errors,
        stop,
        ..
    } = report;
    match stop {
        StopReason::Completed => {
            format!("completed {turns} turn(s) with {errors} error(s)")
        }
        StopReason::TurnLimit => format!(
            "stopped at the --turn-limit of {} with calls still planned ({errors} error(s))",
            plan.turn_limit
        ),
        StopReason::ErrorBudgetExhausted => format!(
            "stopped after {turns} turn(s): {errors} error(s) exceeds the --error-budget of \
             {}. Raise --error-budget to run past them — a server whose tool list includes an \
             error-returning tool (this workspace's `test_error_handling`) needs at least 1",
            plan.error_budget
        ),
        StopReason::Cancelled => {
            format!("cancelled after {turns} turn(s) with {errors} error(s)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_reference_host::run::CallPolicy;

    fn plan(turn_limit: u32, error_budget: u32) -> RunPlan {
        RunPlan {
            turn_limit,
            error_budget,
            calls: CallPolicy::EachDiscoveredToolOnce,
            log_level: None,
            trace_parent: None,
        }
    }

    fn report(stop: StopReason, turns: u32, errors: u32) -> RunReport {
        RunReport {
            turns,
            errors,
            stop,
            outcomes: Vec::new(),
        }
    }

    #[test]
    fn the_error_budget_stop_names_the_flag_and_the_number() {
        let line = stopped(
            &report(StopReason::ErrorBudgetExhausted, 10, 1),
            &plan(20, 0),
        );
        assert!(line.contains("--error-budget of 0"), "{line}");
        assert!(line.contains("1 error(s)"), "{line}");
        assert!(line.contains("Raise --error-budget"), "{line}");
        // The condition, not the Rust variant name.
        assert!(!line.contains("ErrorBudgetExhausted"), "{line}");
    }

    #[test]
    fn every_other_stop_reads_as_a_sentence_too() {
        let done = stopped(&report(StopReason::Completed, 12, 0), &plan(20, 0));
        assert_eq!(done, "completed 12 turn(s) with 0 error(s)");

        let capped = stopped(&report(StopReason::TurnLimit, 20, 0), &plan(20, 0));
        assert!(capped.contains("--turn-limit of 20"), "{capped}");

        let stopped_early = stopped(&report(StopReason::Cancelled, 3, 0), &plan(20, 0));
        assert!(
            stopped_early.starts_with("cancelled after 3"),
            "{stopped_early}"
        );
    }
}
