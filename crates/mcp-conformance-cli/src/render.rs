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
use mcp_trace_validator::report::{HumanOptions, Party, Report, Verdict};
use mcp_trace_validator::{junit, sarif};
use serde_json::json;

use crate::args::{Format, ReportArgs};
use crate::run::Outcome;

mod note;

use note::{failed_scenarios, fixtures_note};

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
    emit_to(&mut std::io::stdout().lock(), text)
}

/// [`emit`], to `out`.
fn emit_to(out: &mut impl std::io::Write, text: &str) -> bool {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => true,
        Err(error) => {
            eprintln!("mcp-conformance: cannot write the report: {error}");
            false
        }
    }
}

/// The run's verdict, from its exit code: `pass` (0), `fail` (1, findings) or
/// `incomplete` (2, a run that could not judge what it recorded).
fn verdict(outcome: &Outcome) -> &'static str {
    match outcome.exit_code() {
        crate::EXIT_OK => "pass",
        crate::EXIT_FINDINGS => "fail",
        _ => "incomplete",
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
    suite_verdict(text, outcome);
    fixtures_note(text, outcome);
}

/// The suite's one-line count and verdict.
fn suite_verdict(text: &mut String, outcome: &Outcome) {
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

/// The other end of the session, as the report names it: the suite's own.
const fn other_end(subject: Party) -> &'static str {
    match subject {
        Party::Server => "the suite's own client",
        Party::Client => "the suite's own test server",
        _ => "the other end",
    }
}

/// The agreement's part of the human report, also the stderr summary's.
fn agreement_part(text: &mut String, outcome: &Outcome) {
    let reconciliation = &outcome.reconciliation;
    let (party, other) = (outcome.party, outcome.party.other());
    let baseline = outcome
        .baseline
        .as_deref()
        .map_or_else(|| "none".to_owned(), |path| path.display().to_string());
    let _ = writeln!(
        text,
        "subject: the {party}; {} is the other end. What is held against it is set aside; what \
         is held against nobody is counted",
        other_end(party)
    );
    let _ = writeln!(text, "divergence baseline: {baseline}");
    let _ = writeln!(
        text,
        "failing clauses: {} ({} explained, {} unexplained, {} set aside); stale baseline \
         entries: {}",
        reconciliation.explained.len()
            + reconciliation.unexplained.len()
            + reconciliation.set_aside.len(),
        reconciliation.explained.len(),
        reconciliation.unexplained.len(),
        reconciliation.set_aside.len(),
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
    if !reconciliation.set_aside.is_empty() {
        let _ = writeln!(
            text,
            "failing clauses held against {} (shown, and not counted):",
            other_end(party)
        );
        for failure in &reconciliation.set_aside {
            let _ = writeln!(
                text,
                "  {} on {} [{other}]: {}",
                failure.requirement, failure.trace, failure.detail
            );
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
    let (party, other) = (outcome.party, outcome.party.other());
    let _ = writeln!(
        text,
        "\n== The clause report: {} recorded session(s) ==",
        judging.judged.len() + judging.unjudged.len()
    );
    let _ = writeln!(
        text,
        "Viewed for the {party}: a finding tagged [{other}] is held against {} and does not \
         count toward a verdict; one tagged [unattributed] is counted.\n",
        other_end(party)
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
    for session in &judging.unjudged {
        let _ = writeln!(
            text,
            "==> {} <==\nnot judged ({}): {}\n",
            session.record.trace,
            class_word(session.class),
            session.record.error
        );
    }
    if !judging.stale_expected.is_empty() {
        let _ = writeln!(
            text,
            "--expected-unjudged names a session that was not recorded, was judged, or went \
             unjudged for another reason — expected to go unjudged for its revision, and did \
             not: {}\n",
            judging.stale_expected.join(", ")
        );
    }
    sessions_line(&mut text, outcome);
    let _ = writeln!(text, "\n== Agreement ==");
    agreement_part(&mut text, outcome);
    let _ = writeln!(text, "\nverdict: {}", verdict(outcome));
    text
}

/// What a class of unjudged session is called in the report.
const fn class_word(class: crate::judging::Class) -> &'static str {
    match class {
        crate::judging::Class::Informational => "informational",
        crate::judging::Class::Error => "error",
    }
}

/// ` (1 error)` when sessions the run could not judge are errors, else nothing.
fn errors_note(judging: &crate::judging::Judging) -> String {
    let errors = judging
        .unjudged
        .iter()
        .filter(|session| session.class == crate::judging::Class::Error)
        .count();
    match errors {
        0 => String::new(),
        1 => " (1 error)".to_owned(),
        many => format!(" ({many} errors)"),
    }
}

/// ` — 6 client findings set aside`, or nothing when the view set none aside
/// and counted no finding held against nobody.
fn what_the_view_did(outcome: &Outcome) -> String {
    let what = outcome.judging.tally(outcome.party).describe(outcome.party);
    if what.is_empty() {
        String::new()
    } else {
        format!(" \u{2014} {what}")
    }
}

/// The sessions by verdict, and the clause verdict they add up to for the
/// subject's party.
fn sessions_line(text: &mut String, outcome: &Outcome) {
    let judging = &outcome.judging;
    let count = |verdict: Verdict| {
        judging
            .judged
            .iter()
            .filter(|one| one.verdict() == verdict)
            .count()
    };
    let _ = writeln!(
        text,
        "{} sessions: {} pass, {} pass-with-warnings, {} fail, {} not judged{}; clause verdict \
         ({} findings only): {}{}",
        judging.judged.len() + judging.unjudged.len(),
        count(Verdict::Pass),
        count(Verdict::PassWithWarnings),
        count(Verdict::Fail),
        judging.unjudged.len(),
        errors_note(judging),
        outcome.party,
        judging.verdict(),
        what_the_view_did(outcome)
    );
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
    let tally = outcome.judging.tally(outcome.party);
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
            // The names the suite asked the server for that its own listings
            // do not include (a server's fixtures are not the suite's), when
            // the suite failed scenarios and the traffic shows any.
            "fixtures": outcome.fixtures.as_ref().map(|fixtures| json!({
                "failed_scenarios": failed_scenarios(outcome),
                "scenarios": outcome.scenarios.len(),
                "sessions": fixtures.sessions,
                "judged_sessions": fixtures.judged_sessions,
                "unlisted": fixtures.unlisted,
            })),
        },
        "sessions": {
            // Each session's reports are viewed for the subject's party, and
            // so is this: what is held against the suite's own end does not
            // move it (`findings.set_aside` counts what that was).
            "party": outcome.party,
            "verdict": outcome.judging.verdict(),
            "findings": {
                "counted": tally.counted,
                "set_aside": tally.set_aside,
                "unattributed": tally.unattributed,
            },
            "totals": outcome.judging.totals(),
            "traces": traces,
            "unjudged": outcome.judging.unjudged,
            "stale_expected": outcome.judging.stale_expected,
        },
        "agreement": {
            "baseline": outcome.baseline.as_deref().map(|path| path.display().to_string()),
            "party": outcome.party,
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
    junit::render_run(&suites, &outcome.judging.errors())
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
        &outcome.judging.errors(),
        &sarif::Options::default(),
    )
}

/// Whether the report as written already holds the suite's checks and the
/// agreement in full *where a reader of the terminal has it*: the human and
/// JSON reports, on stdout. Written to a file, or as `junit` or `sarif` (which
/// carry only the clause report), they are not, and stderr has them.
const fn report_on_stdout_carries_all(report: &ReportArgs) -> bool {
    report.output.is_none() && matches!(report.format, Format::Human | Format::Json)
}

/// The clause verdict, for the subject's party, in one line.
fn clause_verdict(text: &mut String, outcome: &Outcome) {
    let _ = writeln!(
        text,
        "clause verdict ({} findings only) over {} judged session(s): {}{}",
        outcome.party,
        outcome.judging.judged.len(),
        outcome.judging.verdict(),
        what_the_view_did(outcome)
    );
}

/// Whether the agreement holds, in one line, for a reader who has the report.
fn agreement_verdict(text: &mut String, outcome: &Outcome) {
    let reconciliation = &outcome.reconciliation;
    if reconciliation.holds() {
        let _ = writeln!(text, "agreement: holds");
    } else {
        let _ = writeln!(
            text,
            "agreement: does not hold ({} unexplained, {} stale baseline entries; the report \
             says which)",
            reconciliation.unexplained.len(),
            reconciliation.stale.len()
        );
    }
}

/// The run in a few lines on stderr: the suite's checks and the agreement in
/// full when the report does not carry them where the terminal shows it, in a
/// line each when it does (the same block twice, one after the other, was the
/// fault); the two verdicts; and where everything is kept.
pub(crate) fn summary(outcome: &Outcome, report: &ReportArgs) {
    let mut text = String::new();
    if report_on_stdout_carries_all(report) {
        suite_verdict(&mut text, outcome);
        clause_verdict(&mut text, outcome);
        agreement_verdict(&mut text, outcome);
    } else {
        suite_part(&mut text, outcome);
        clause_verdict(&mut text, outcome);
        agreement_part(&mut text, outcome);
    }
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
