// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Several traces' reports as one document per format.
//!
//! A trace the run could not judge is part of the run's report, not only of
//! its stderr: CI uploads the `JUnit` file and the SARIF log and reads the
//! JSON, and none of them has a terminal behind it. Each format lists every
//! such trace with its reason, and the overall verdict reads
//! [`Verdict::Incomplete`] — never the `pass` the judged traces alone might
//! earn, which is what the run said before while exiting `3`.

use std::fmt::Write as _;

use mcp_trace_validator::report::{Report, Unjudged, Verdict};
use mcp_trace_validator::{html, junit, sarif};
use serde_json::json;

use super::{
    Judged, Output, artifact, junit_options, note_strict, page_trace, promoted, verdict_to_code,
};
use crate::emit::emit;
use crate::{EXIT_OK, Format};

/// Renders the run; `None` when it could not be written.
pub(super) fn render(judged: &[Judged], unjudged: &[Unjudged], output: Output) -> Option<u8> {
    let overall = overall(judged, unjudged);
    let written = match output.format {
        Format::Human => emit(&human(judged, unjudged, overall, output)),
        Format::Json => emit(&json(judged, unjudged, overall)?),
        Format::Junit => {
            let suites: Vec<(junit::Options, &[Report])> = judged
                .iter()
                .map(|one| {
                    (
                        junit_options(&one.trace, output.strict),
                        one.reports.as_slice(),
                    )
                })
                .collect();
            emit(&junit::render_run(&suites, unjudged))
        }
        Format::Sarif => {
            let traces: Vec<(sarif::Artifact<'_>, &[Report])> = judged
                .iter()
                .map(|one| (artifact(&one.trace, &one.events), one.reports.as_slice()))
                .collect();
            emit(&sarif::render_run(
                &traces,
                unjudged,
                &sarif::Options::default().strict(output.strict),
            ))
        }
        Format::Html => {
            let traces: Vec<(html::Trace<'_>, &[Report])> = judged
                .iter()
                .map(|one| (page_trace(&one.trace, &one.events), one.reports.as_slice()))
                .collect();
            emit(&html::render_run(&traces, unjudged))
        }
    };
    written.then(|| {
        let promoted: Vec<&str> = judged
            .iter()
            .filter(|one| promoted(one.verdict(), output.strict))
            .map(|one| one.trace.as_str())
            .collect();
        if !promoted.is_empty() {
            note_strict(&format!(" for {}", promoted.join(", ")));
        }
        judged
            .iter()
            .map(|one| verdict_to_code(one.verdict(), output.strict))
            .max()
            .unwrap_or(EXIT_OK)
    })
}

/// The run's verdict: the most severe of the judged traces', or incomplete
/// when any trace could not be judged.
fn overall(judged: &[Judged], unjudged: &[Unjudged]) -> Verdict {
    let incomplete = unjudged.first().map(|_| Verdict::Incomplete);
    Verdict::most_severe(judged.iter().map(Judged::verdict).chain(incomplete))
        .unwrap_or(Verdict::Incomplete)
}

/// Each judged trace's human report under a `==> trace <==` header, then each
/// unjudged trace's reason under its own, then how many reached each verdict
/// and the overall one.
fn human(judged: &[Judged], unjudged: &[Unjudged], overall: Verdict, output: Output) -> String {
    let mut text = String::new();
    for one in judged {
        let _ = writeln!(text, "==> {} <==", one.trace);
        text.push_str(&one.render_human(output));
        text.push('\n');
    }
    for trace in unjudged {
        let _ = writeln!(text, "==> {} <==", trace.trace);
        let _ = writeln!(text, "not judged: {}", trace.error);
        text.push('\n');
    }
    text.push_str(&summary(judged, unjudged.len()));
    let _ = writeln!(text, "overall verdict: {overall}");
    text
}

/// Several traces' JSON reports in one document: `{"verdict", "traces":
/// [{"trace", "report"}], "unjudged", "unjudged_traces"}`
/// (`report.schema.json`'s third shape). `unjudged_traces` lists each trace
/// that could not be judged with its reason, and is absent when there is
/// none. `None` when a report could not be serialized.
fn json(judged: &[Judged], unjudged: &[Unjudged], overall: Verdict) -> Option<String> {
    let mut traces = Vec::new();
    for one in judged {
        match one.to_json() {
            Ok(report) => traces.push(json!({ "trace": one.trace, "report": report })),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return None;
            }
        }
    }
    let mut document = json!({ "verdict": overall, "traces": traces, "unjudged": unjudged.len() });
    if !unjudged.is_empty() {
        document["unjudged_traces"] = json!(unjudged);
    }
    match serde_json::to_string_pretty(&document) {
        Ok(json) => Some(format!("{json}\n")),
        Err(error) => {
            eprintln!("error: cannot serialize report: {error}");
            None
        }
    }
}

/// The closing line of a multi-trace human report: how many traces reached
/// each verdict, and how many could not be judged at all.
fn summary(judged: &[Judged], unjudged: usize) -> String {
    let count = |verdict: Verdict| judged.iter().filter(|one| one.verdict() == verdict).count();
    let mut line = format!(
        "{} traces: {} pass, {} pass-with-warnings, {} fail",
        judged.len() + unjudged,
        count(Verdict::Pass),
        count(Verdict::PassWithWarnings),
        count(Verdict::Fail),
    );
    if unjudged > 0 {
        let _ = write!(line, ", {unjudged} not judged (see the errors above)");
    }
    line.push('\n');
    line
}
