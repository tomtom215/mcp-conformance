// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Every recorded session, judged as `mcp-trace-validator validate` judges it:
//! at the revision it declares ([`judge::judge`]), with a session that cannot
//! be judged — the server never answered, nothing judgeable — kept, with its
//! reason, rather than dropped.
//!
//! A refused session is not by itself a failure of the run: the suite's
//! `dns-rebinding-protection` scenario, for one, sends requests a correct
//! server refuses, and their recording holds nothing to judge.

use std::path::{Path, PathBuf};

use mcp_conformance_cli::agreement::{self, Totals, ValidatorFailure};
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::judge;
use mcp_trace_validator::report::{Report, Unjudged, Verdict};

/// One judged session.
#[derive(Debug)]
pub(crate) struct Judged {
    /// Its name: the path under the trace directory, `/`-separated.
    pub(crate) name: String,
    /// Its file.
    pub(crate) path: PathBuf,
    /// Its events, for SARIF's line numbers.
    pub(crate) events: Vec<TraceEvent>,
    /// One report per revision it was judged at.
    pub(crate) reports: Vec<Report>,
}

impl Judged {
    pub(crate) fn verdict(&self) -> Verdict {
        Verdict::most_severe(self.reports.iter().map(Report::verdict)).unwrap_or(Verdict::Pass)
    }
}

/// Every session of a run.
#[derive(Debug, Default)]
pub(crate) struct Judging {
    pub(crate) judged: Vec<Judged>,
    /// Sessions that could not be judged, by path, with why.
    pub(crate) unjudged: Vec<Unjudged>,
}

impl Judging {
    /// The most severe verdict across the judged sessions.
    pub(crate) fn verdict(&self) -> Verdict {
        Verdict::most_severe(self.judged.iter().map(Judged::verdict)).unwrap_or(Verdict::Pass)
    }

    /// Every failing clause, as the agreement check counts them.
    pub(crate) fn failures(&self) -> Vec<ValidatorFailure> {
        self.judged
            .iter()
            .flat_map(|one| {
                one.reports
                    .iter()
                    .flat_map(|report| agreement::failures(&one.name, report))
            })
            .collect()
    }

    /// Outcome counts across every report.
    pub(crate) fn totals(&self) -> Totals {
        let mut totals = Totals::default();
        for report in self.judged.iter().flat_map(|one| &one.reports) {
            totals.add(report);
        }
        totals
    }
}

/// Judges every `*.jsonl` in `dir` and the directories directly under it, in
/// name order.
///
/// # Errors
///
/// A directory cannot be listed.
pub(crate) fn judge_dir(dir: &Path) -> Result<Judging, String> {
    let mut judging = Judging::default();
    for (name, path) in traces(dir)? {
        match std::fs::read_to_string(&path) {
            Err(error) => judging.unjudged.push(Unjudged::new(
                path.display().to_string(),
                format!("cannot read: {error}"),
            )),
            Ok(document) => match judge::judge(&document) {
                Ok(judgment) => judging.judged.push(Judged {
                    name,
                    path,
                    events: judgment.events,
                    reports: judgment.reports,
                }),
                Err(error) => judging
                    .unjudged
                    .push(Unjudged::new(path.display().to_string(), error.to_string())),
            },
        }
    }
    Ok(judging)
}

/// The traces under `dir`, one level of subdirectories deep, sorted by name.
fn traces(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut found = Vec::new();
    for (name, path) in entries(dir)? {
        if path.is_dir() {
            for (inner, path) in entries(&path)? {
                if is_trace(&path) {
                    found.push((format!("{name}/{inner}"), path));
                }
            }
        } else if is_trace(&path) {
            found.push((name, path));
        }
    }
    found.sort();
    Ok(found)
}

fn entries(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let listing = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    listing
        .map(|entry| {
            entry
                .map(|entry| {
                    (
                        entry.file_name().to_string_lossy().into_owned(),
                        entry.path(),
                    )
                })
                .map_err(|error| format!("{}: {error}", dir.display()))
        })
        .collect()
}

fn is_trace(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const CLEAN: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
"#;

    /// The clean session, then a request id reused: BASE-003 fails.
    fn failing() -> String {
        format!(
            "{CLEAN}{}\n{}\n",
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}"#,
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#
        )
    }

    #[test]
    fn every_trace_is_judged_or_kept_with_its_reason() {
        let dir =
            std::env::temp_dir().join(format!("mcp-conformance-judging-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("initialize")).unwrap();
        std::fs::write(dir.join("002.jsonl"), failing()).unwrap();
        std::fs::write(dir.join("001.jsonl"), CLEAN).unwrap();
        std::fs::write(dir.join("initialize/001.jsonl"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a trace").unwrap();
        std::fs::create_dir_all(dir.join("initialize/deeper")).unwrap();
        std::fs::write(dir.join("initialize/deeper/001.jsonl"), CLEAN).unwrap();

        let judging = judge_dir(&dir).unwrap();
        let names: Vec<&str> = judging.judged.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, ["001.jsonl", "002.jsonl"]);
        assert_eq!(judging.judged[0].verdict(), Verdict::Pass);
        assert_eq!(judging.judged[1].verdict(), Verdict::Fail);
        assert_eq!(judging.verdict(), Verdict::Fail);
        assert_eq!(judging.unjudged.len(), 1);
        assert!(judging.unjudged[0].trace.ends_with("001.jsonl"));
        assert!(
            judging.unjudged[0].error.contains("judged no requirement"),
            "{:?}",
            judging.unjudged
        );
        let failures = judging.failures();
        assert!(failures.iter().all(|failure| failure.trace == "002.jsonl"));
        assert!(
            failures
                .iter()
                .any(|failure| failure.requirement == "BASE-003")
        );
        let totals = judging.totals();
        assert_eq!(
            totals.fail,
            judging.judged[1].reports[0].totals.fail + judging.judged[0].reports[0].totals.fail
        );
        assert!(totals.pass > judging.judged[0].reports[0].totals.pass);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(judge_dir(&dir).unwrap_err().starts_with("cannot read "));
    }

    #[test]
    fn nothing_judged_passes_vacuously_only_as_a_verdict() {
        // The run decides what an empty judging means; the verdict alone is pass.
        assert_eq!(Judging::default().verdict(), Verdict::Pass);
        assert_eq!(Judging::default().failures().len(), 0);
    }
}
