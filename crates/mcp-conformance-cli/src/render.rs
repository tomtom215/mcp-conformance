// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The report, in the validator's four formats.
//!
//! `human` and `json` carry all three parts: the suite's checks by scenario,
//! the clause report of every recorded session (the validator's own human
//! rendering and JSON report shape), and the agreement. `junit` and `sarif`
//! are the validator's renderers over the recorded sessions, unchanged, so a CI
//! system that ingests the validator's reports ingests these; the suite's
//! checks and the agreement are on stderr ([`summary`]) whatever the format.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use mcp_conformance_cli::suite::{self, Scenario, Summary};
use mcp_trace_validator::report::{HumanOptions, Report, Verdict};
use mcp_trace_validator::{junit, sarif};
use serde_json::json;

use crate::args::{Format, ReportArgs};
use crate::run::Outcome;

/// The most findings the default human report lists under one clause, as the
/// validator's own default.
const MAX_FINDINGS: usize = 20;

/// Renders the report and writes it to `--output` or stdout. Returns whether
/// it was written.
pub(crate) fn write(outcome: &Outcome, args: &ReportArgs) -> bool {
    let text = match args.format {
        Format::Human => human(outcome, args.all),
        Format::Json => match json_report(outcome) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("mcp-conformance: cannot serialize the report: {error}");
                return false;
            }
        },
        Format::Junit => junit_report(outcome),
        Format::Sarif => sarif_report(outcome),
    };
    match &args.output {
        Some(path) => std::fs::write(path, text).map_or_else(
            |error| {
                eprintln!("mcp-conformance: cannot write {}: {error}", path.display());
                false
            },
            |()| true,
        ),
        None => emit(&text),
    }
}

/// Writes `text` to stdout. A reader that closed the pipe early has what it
/// wanted; any other failure means the report is missing.
fn emit(text: &str) -> bool {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => true,
        Err(error) => {
            eprintln!("mcp-conformance: cannot write the report: {error}");
            false
        }
    }
}

/// The run's verdict: `pass` exactly when it exits 0.
fn verdict(outcome: &Outcome) -> &'static str {
    if outcome.exit_code() == 0 {
        "pass"
    } else {
        "fail"
    }
}

/// `SUCCESS 3, FAILURE 1` — a status count in the suite's own words.
fn counts(by_status: &BTreeMap<String, u32>) -> String {
    let parts: Vec<String> = by_status
        .iter()
        .map(|(status, count)| format!("{count} {status}"))
        .collect();
    parts.join(", ")
}

fn scenario_line(text: &mut String, scenario: &Scenario) {
    let summary = Summary::of(std::slice::from_ref(scenario));
    let mark = if scenario.failed() { "FAIL" } else { "pass" };
    let _ = writeln!(
        text,
        "  {mark}  {:<40} {}",
        scenario.name,
        counts(&summary.checks_by_status)
    );
    for check in &scenario.checks {
        if matches!(check.status.as_str(), "FAILURE" | "WARNING") {
            let message = check.error_message.as_deref().unwrap_or("(no message)");
            let _ = writeln!(text, "          {} {}: {message}", check.status, check.id);
        }
    }
}

/// The suite's part of the human report.
fn suite_part(text: &mut String, outcome: &Outcome) {
    let _ = writeln!(text, "== The official suite ==");
    for scenario in &outcome.scenarios {
        scenario_line(text, scenario);
    }
    for (scenario, why) in &outcome.not_run {
        let _ = writeln!(text, "  --    {scenario:<40} not run: {why}");
    }
    let summary = Summary::of(&outcome.scenarios);
    let _ = writeln!(
        text,
        "{} scenarios, {} checks: {}; suite verdict: {}",
        summary.scenarios,
        summary.checks,
        counts(&summary.checks_by_status),
        if outcome.suite_passed { "pass" } else { "fail" }
    );
}

/// The agreement's part of the human report, also the stderr summary's.
fn agreement_part(text: &mut String, outcome: &Outcome) {
    let reconciliation = &outcome.reconciliation;
    let baseline = outcome
        .baseline
        .as_deref()
        .map_or_else(|| "none".to_owned(), |path| path.display().to_string());
    let _ = writeln!(text, "divergence baseline: {baseline}");
    let _ = writeln!(
        text,
        "failing clauses: {} ({} explained, {} unexplained); stale baseline entries: {}",
        reconciliation.explained.len() + reconciliation.unexplained.len(),
        reconciliation.explained.len(),
        reconciliation.unexplained.len(),
        reconciliation.stale.len()
    );
    let name = outcome.baseline.as_deref().map_or_else(
        || "a baseline (--divergences)".to_owned(),
        |p| p.display().to_string(),
    );
    match reconciliation.gate_error(&name) {
        None => {
            let _ = writeln!(text, "agreement: holds");
        }
        Some(why) => {
            let _ = writeln!(text, "agreement: does not hold\n{why}");
        }
    }
}

/// The human report.
pub(crate) fn human(outcome: &Outcome, all: bool) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "mcp-conformance test: {}\nofficial suite {}@{}, spec {}\n",
        outcome.subject,
        suite::PACKAGE,
        suite::VERSION,
        outcome.spec_version
    );
    suite_part(&mut text, outcome);
    let judging = &outcome.judging;
    let _ = writeln!(
        text,
        "\n== The clause report: {} recorded session(s) ==",
        judging.judged.len() + judging.unjudged.len()
    );
    let options = HumanOptions::default()
        .all(all)
        .max_findings((!all).then_some(MAX_FINDINGS));
    for one in &judging.judged {
        let _ = writeln!(text, "==> {} <==", one.path.display());
        for report in &one.reports {
            text.push_str(&report.render_with(&options));
        }
        text.push('\n');
    }
    for trace in &judging.unjudged {
        let _ = writeln!(
            text,
            "==> {} <==\nnot judged: {}\n",
            trace.trace, trace.error
        );
    }
    let count = |verdict: Verdict| {
        judging
            .judged
            .iter()
            .filter(|one| one.verdict() == verdict)
            .count()
    };
    let _ = writeln!(
        text,
        "{} sessions: {} pass, {} pass-with-warnings, {} fail, {} not judged; clause verdict: {}",
        judging.judged.len() + judging.unjudged.len(),
        count(Verdict::Pass),
        count(Verdict::PassWithWarnings),
        count(Verdict::Fail),
        judging.unjudged.len(),
        judging.verdict()
    );
    let _ = writeln!(text, "\n== Agreement ==");
    agreement_part(&mut text, outcome);
    let _ = writeln!(text, "\nverdict: {}", verdict(outcome));
    text
}

/// The JSON report.
pub(crate) fn json_report(outcome: &Outcome) -> serde_json::Result<String> {
    let mut traces = Vec::new();
    for one in &outcome.judging.judged {
        traces.push(json!({
            "trace": one.path.display().to_string(),
            "reports": one.reports.iter().map(serde_json::to_value).collect::<Result<Vec<_>, _>>()?,
        }));
    }
    let not_run: Vec<_> = outcome
        .not_run
        .iter()
        .map(|(scenario, reason)| json!({"scenario": scenario, "reason": reason}))
        .collect();
    let document = json!({
        "subject": outcome.subject,
        "work_dir": outcome.work_dir.display().to_string(),
        "suite": {
            "package": suite::PACKAGE,
            "version": suite::VERSION,
            "spec_version": outcome.spec_version,
            "passed": outcome.suite_passed,
            "summary": Summary::of(&outcome.scenarios),
            "scenarios": outcome.scenarios,
            "not_run": not_run,
        },
        "sessions": {
            "verdict": outcome.judging.verdict(),
            "totals": outcome.judging.totals(),
            "traces": traces,
            "unjudged": outcome.judging.unjudged,
        },
        "agreement": {
            "baseline": outcome.baseline.as_deref().map(|path| path.display().to_string()),
            "holds": outcome.reconciliation.holds(),
            "divergences": outcome.reconciliation,
        },
        "verdict": verdict(outcome),
    });
    serde_json::to_string_pretty(&document).map(|text| text + "\n")
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn junit_report(outcome: &Outcome) -> String {
    let judged = &outcome.judging.judged;
    let names: Vec<String> = judged.iter().map(|one| path_text(&one.path)).collect();
    let suites: Vec<(junit::Options, &[Report])> = judged
        .iter()
        .zip(&names)
        .map(|(one, name)| {
            (
                junit::Options::for_trace(name.as_str()),
                one.reports.as_slice(),
            )
        })
        .collect();
    junit::render_run(&suites, &outcome.judging.unjudged)
}

fn sarif_report(outcome: &Outcome) -> String {
    let judged = &outcome.judging.judged;
    let names: Vec<String> = judged.iter().map(|one| path_text(&one.path)).collect();
    let traces: Vec<(sarif::Artifact<'_>, &[Report])> = judged
        .iter()
        .zip(&names)
        .map(|(one, name)| {
            (
                sarif::Artifact {
                    uri: Some(name.as_str()),
                    events: &one.events,
                },
                one.reports.as_slice(),
            )
        })
        .collect();
    sarif::render_run(
        &traces,
        &outcome.judging.unjudged,
        &sarif::Options::default(),
    )
}

/// The run in a few lines on stderr: the suite's checks when the report does
/// not carry them, then the two verdicts, the agreement, and where everything
/// is kept.
pub(crate) fn summary(outcome: &Outcome) {
    let mut text = String::new();
    suite_part(&mut text, outcome);
    let _ = writeln!(
        text,
        "clause verdict over {} judged session(s): {}",
        outcome.judging.judged.len(),
        outcome.judging.verdict()
    );
    agreement_part(&mut text, outcome);
    let _ = writeln!(
        text,
        "verdict: {}; suite results and traces kept in {}",
        verdict(outcome),
        outcome.work_dir.display()
    );
    for line in text.lines() {
        eprintln!("mcp-conformance: {line}");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
