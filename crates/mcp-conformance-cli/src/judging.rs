// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Every recorded session, judged as `mcp-trace-validator validate` judges it:
//! at the revision it declares ([`judge::judge`]), with a session that cannot
//! be judged kept, with its reason and its [`Class`], rather than dropped.
//!
//! A session that cannot be judged is one of two things. **Informational**: the
//! server never answered, or nothing was judged — the suite's
//! `dns-rebinding-protection` scenario sends requests a correct server refuses,
//! and their recording holds nothing to judge — so the run has lost nothing.
//! **An error**: a trace that is malformed or unreadable, a build that cannot
//! load its registry, or a session that ran under a revision this build has no
//! registry for, which the run recorded and could not judge. The last is an
//! error unless the caller names the session as expected
//! (`--expected-unjudged`), because a pass over a session nobody judged is the
//! failure this separates from a pass.
//!
//! **Viewed for the subject.** The suite's own client (or, for a client under
//! test, its own server) breaks clauses of its own, so every report is viewed
//! for the subject's party, as `mcp-trace-validator validate --party` views
//! it: the rows and totals stay the trace's facts, each finding keeps the
//! party it is held against, and the verdict counts the subject's findings and
//! those held against nobody.

use std::path::{Path, PathBuf};

use mcp_conformance_cli::agreement::{self, Totals, ValidatorFailure};
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::judge::{self, JudgeError};
use mcp_trace_validator::report::{Party, PartyTally, Report, Unjudged, Verdict};
use serde::Serialize;

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
    /// The session's verdict as its reports are viewed ([`Report::effective_verdict`]):
    /// for the subject's party, when the session was judged with one.
    pub(crate) fn verdict(&self) -> Verdict {
        Verdict::most_severe(self.reports.iter().map(Report::effective_verdict))
            .unwrap_or(Verdict::Pass)
    }
}

/// What a session that could not be judged means for the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Class {
    /// Nothing to judge was recorded: the run has lost nothing.
    Informational,
    /// The run recorded a session and could not judge it: its verdict is
    /// incomplete.
    Error,
}

/// A session that could not be judged: why, and what that means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct UnjudgedSession {
    /// Its path and the reason, as every format lists it.
    #[serde(flatten)]
    pub(crate) record: Unjudged,
    /// Its name under the trace directory (`initialize/001.jsonl`), which
    /// `--expected-unjudged` matches.
    pub(crate) name: String,
    pub(crate) class: Class,
}

/// Every session of a run.
#[derive(Debug, Default)]
pub(crate) struct Judging {
    pub(crate) judged: Vec<Judged>,
    /// Sessions that could not be judged, with why and what it means.
    pub(crate) unjudged: Vec<UnjudgedSession>,
    /// Names given as expected to go unjudged for their revision that did not:
    /// the session was judged, went unjudged for another reason, or is absent.
    pub(crate) stale_expected: Vec<String>,
}

impl Judging {
    /// Whether the run recorded a session it could not judge, or was told to
    /// expect one that did not occur: its verdict is incomplete.
    pub(crate) fn has_errors(&self) -> bool {
        !self.stale_expected.is_empty()
            || self
                .unjudged
                .iter()
                .any(|session| session.class == Class::Error)
    }

    /// The sessions that could not be judged and are an error, as the
    /// machine formats list them.
    pub(crate) fn errors(&self) -> Vec<Unjudged> {
        self.unjudged
            .iter()
            .filter(|session| session.class == Class::Error)
            .map(|session| session.record.clone())
            .collect()
    }

    /// The most severe verdict across the judged sessions, each as its
    /// reports are viewed.
    pub(crate) fn verdict(&self) -> Verdict {
        Verdict::most_severe(self.judged.iter().map(Judged::verdict)).unwrap_or(Verdict::Pass)
    }

    /// Every failing clause, once for each party its findings are held
    /// against: what the agreement check splits between the subject and the
    /// suite's own end ([`agreement::reconcile_for`]).
    pub(crate) fn failures(&self) -> Vec<ValidatorFailure> {
        self.judged
            .iter()
            .flat_map(|one| {
                one.reports
                    .iter()
                    .flat_map(|report| agreement::failures_by_party(&one.name, report))
            })
            .collect()
    }

    /// How every report's findings fall when viewed for `party`, summed.
    pub(crate) fn tally(&self, party: Party) -> PartyTally {
        let mut sum = PartyTally::default();
        for report in self.judged.iter().flat_map(|one| &one.reports) {
            let tally = report.party_tally(party);
            sum.counted += tally.counted;
            sum.set_aside += tally.set_aside;
            sum.unattributed += tally.unattributed;
        }
        sum
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

/// Whether `expected` names the session `name`: exactly, or as the directory it
/// is in.
fn names(expected: &str, name: &str) -> bool {
    name == expected
        || name
            .strip_prefix(expected)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Judges every `*.jsonl` in `dir` and the directories directly under it, in
/// name order, each report viewed for `subject`. A session in `expected` that
/// goes unjudged for its revision is informational rather than an error.
///
/// # Errors
///
/// A directory cannot be listed.
pub(crate) fn judge_dir(
    dir: &Path,
    subject: Party,
    expected: &[String],
) -> Result<Judging, String> {
    let mut judging = Judging::default();
    let mut matched = vec![false; expected.len()];
    for (name, path) in traces(dir)? {
        let unjudged = |class, error: String| UnjudgedSession {
            record: Unjudged::new(path.display().to_string(), error),
            name: name.clone(),
            class,
        };
        match std::fs::read_to_string(&path) {
            Err(error) => judging
                .unjudged
                .push(unjudged(Class::Error, format!("cannot read: {error}"))),
            Ok(document) => match judge::judge(&document) {
                Ok(judgment) => judging.judged.push(Judged {
                    name,
                    path,
                    events: judgment.events,
                    // The view is applied after judging, as `validate --party`
                    // does: it changes what counts, never what was found.
                    reports: judgment
                        .reports
                        .into_iter()
                        .map(|report| viewed_for(report, subject))
                        .collect(),
                }),
                Err(error) => {
                    let named: Vec<usize> = (0..expected.len())
                        .filter(|&index| names(&expected[index], &name))
                        .collect();
                    let unsupported = matches!(error, JudgeError::UnsupportedRevision(_));
                    if unsupported {
                        for &index in &named {
                            matched[index] = true;
                        }
                    }
                    let class = class_of(&error, unsupported && !named.is_empty());
                    judging.unjudged.push(unjudged(class, error.to_string()));
                }
            },
        }
    }
    judging.stale_expected = expected
        .iter()
        .zip(matched)
        .filter(|&(_, matched)| !matched)
        .map(|(name, _)| name.clone())
        .collect();
    Ok(judging)
}

/// What `error` means for the run. A reason this build does not know is an
/// error, so a new way to go unjudged fails the run until it is classified.
const fn class_of(error: &JudgeError, expected_revision: bool) -> Class {
    match error {
        JudgeError::NeverAnswered(_) | JudgeError::NothingJudged => Class::Informational,
        JudgeError::UnsupportedRevision(_) if expected_revision => Class::Informational,
        _ => Class::Error,
    }
}

/// `report`, viewed for `party` (`validate --party`).
const fn viewed_for(mut report: Report, party: Party) -> Report {
    report.party_view = Some(party);
    report
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

        // BASE-003 is the client's finding, so it is a client under test whose
        // verdict it moves.
        let judging = judge_dir(&dir, Party::Client, &[]).unwrap();
        let names: Vec<&str> = judging.judged.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, ["001.jsonl", "002.jsonl"]);
        assert_eq!(judging.judged[0].verdict(), Verdict::Pass);
        assert_eq!(judging.judged[1].verdict(), Verdict::Fail);
        assert_eq!(judging.verdict(), Verdict::Fail);
        assert!(
            judging
                .judged
                .iter()
                .flat_map(|one| &one.reports)
                .all(|report| report.party_view == Some(Party::Client)),
            "every report is viewed for the subject"
        );
        assert_eq!(judging.unjudged.len(), 1);
        assert!(judging.unjudged[0].record.trace.ends_with("001.jsonl"));
        assert!(
            judging.unjudged[0]
                .record
                .error
                .contains("judged no requirement"),
            "{:?}",
            judging.unjudged
        );
        let failures = judging.failures();
        assert!(failures.iter().all(|failure| failure.trace == "002.jsonl"));
        assert!(failures.iter().any(
            |failure| failure.requirement == "BASE-003" && failure.party == Some(Party::Client)
        ));
        let totals = judging.totals();
        assert_eq!(
            totals.fail,
            judging.judged[1].reports[0].totals.fail + judging.judged[0].reports[0].totals.fail
        );
        assert!(totals.pass > judging.judged[0].reports[0].totals.pass);

        // The same traces for a server under test: the clause row is still
        // the trace's fact, and the verdict is not moved by the client's id.
        let server = judge_dir(&dir, Party::Server, &[]).unwrap();
        assert_eq!(server.judged[1].verdict(), Verdict::Pass);
        assert_eq!(server.verdict(), Verdict::Pass);
        assert_eq!(server.judged[1].reports[0].totals.fail, 1);
        assert_eq!(server.failures(), judging.failures());
        let tally = server.tally(Party::Server);
        assert_eq!((tally.counted, tally.set_aside), (0, 1), "{tally:?}");
        let tally = judging.tally(Party::Client);
        assert_eq!((tally.counted, tally.set_aside), (1, 0), "{tally:?}");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            judge_dir(&dir, Party::Server, &[])
                .unwrap_err()
                .starts_with("cannot read ")
        );
    }

    #[test]
    fn nothing_judged_passes_vacuously_only_as_a_verdict() {
        // The run decides what an empty judging means; the verdict alone is pass.
        assert_eq!(Judging::default().verdict(), Verdict::Pass);
        assert_eq!(Judging::default().failures().len(), 0);
    }

    /// The client leg's own session: the server negotiated 2025-03-26, which no
    /// registry here describes.
    fn negotiated_unsupported() -> String {
        CLEAN.replace(
            r#""result":{"protocolVersion":"2025-11-25""#,
            r#""result":{"protocolVersion":"2025-03-26""#,
        )
    }

    /// A directory holding one session per kind of way a session goes unjudged.
    fn unjudged_kinds(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mcp-conformance-judging-{name}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("unsupported")).unwrap();
        std::fs::write(dir.join("clean.jsonl"), CLEAN).unwrap();
        std::fs::write(dir.join("empty.jsonl"), "").unwrap();
        std::fs::write(
            dir.join("silent.jsonl"),
            CLEAN.lines().next().unwrap().to_owned() + "\n",
        )
        .unwrap();
        std::fs::write(dir.join("malformed.jsonl"), "this is not a trace\n").unwrap();
        std::fs::write(dir.join("unreadable.jsonl"), [0xff_u8, 0xfe, 0x00]).unwrap();
        std::fs::write(dir.join("unsupported/001.jsonl"), negotiated_unsupported()).unwrap();
        dir
    }

    fn classes(judging: &Judging) -> Vec<(&str, Class)> {
        judging
            .unjudged
            .iter()
            .map(|session| (session.name.as_str(), session.class))
            .collect()
    }

    /// A session nothing could be judged from — the server never answered, or
    /// the trace holds nothing to judge — is informational. A trace that is
    /// malformed or unreadable, and a session under a revision this build has
    /// no registry for, is an error: the run could not judge what it recorded.
    #[test]
    fn an_unjudged_session_is_informational_or_an_error_by_why() {
        let dir = unjudged_kinds("classes");
        let judging = judge_dir(&dir, Party::Server, &[]).unwrap();
        assert_eq!(
            classes(&judging),
            [
                ("empty.jsonl", Class::Informational),
                ("malformed.jsonl", Class::Error),
                ("silent.jsonl", Class::Informational),
                ("unreadable.jsonl", Class::Error),
                ("unsupported/001.jsonl", Class::Error),
            ]
        );
        assert!(judging.has_errors());
        assert_eq!(judging.judged.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A session named as expected to go unjudged for its revision is
    /// informational, by exact name or as a directory's; naming one that did
    /// not go unjudged that way is stale, and says so.
    #[test]
    fn an_unsupported_revision_is_informational_only_when_named_as_expected() {
        let dir = unjudged_kinds("expected");
        for expected in ["unsupported/001.jsonl", "unsupported"] {
            let judging = judge_dir(&dir, Party::Server, &[expected.to_owned()]).unwrap();
            let unsupported = judging
                .unjudged
                .iter()
                .find(|session| session.name == "unsupported/001.jsonl")
                .unwrap();
            assert_eq!(unsupported.class, Class::Informational, "{expected}");
            assert!(judging.stale_expected.is_empty(), "{expected}");
        }
        // A name is not a prefix of a longer name, and naming a session that
        // was judged, malformed, or absent expects nothing that happened.
        let judging = judge_dir(
            &dir,
            Party::Server,
            &[
                "unsup".to_owned(),
                "clean.jsonl".to_owned(),
                "malformed.jsonl".to_owned(),
            ],
        )
        .unwrap();
        assert_eq!(
            judging.stale_expected,
            ["unsup", "clean.jsonl", "malformed.jsonl"]
        );
        assert_eq!(
            judging
                .unjudged
                .iter()
                .find(|session| session.name == "malformed.jsonl")
                .unwrap()
                .class,
            Class::Error,
            "only an unsupported revision can be expected"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_run_with_only_informational_sessions_has_no_errors() {
        let dir = unjudged_kinds("clean");
        std::fs::remove_file(dir.join("malformed.jsonl")).unwrap();
        std::fs::remove_file(dir.join("unreadable.jsonl")).unwrap();
        let judging = judge_dir(&dir, Party::Server, &["unsupported".to_owned()]).unwrap();
        assert!(!judging.has_errors(), "{:?}", classes(&judging));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
