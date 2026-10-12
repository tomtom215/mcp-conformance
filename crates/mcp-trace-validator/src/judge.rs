// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Judging a trace document in one call, by the rules the CLI applies.
//!
//! The building blocks ([`reader`], [`declared`], [`engine`]) stay available for
//! callers who need to vary a step. This module is for the common case — "is this
//! recording conformant?" — and gives the same answer `mcp-trace-validator
//! validate` does: the revision the trace declares (else the newest), and a
//! refusal, not a vacuous pass, for a trace that judges nothing or records a
//! session the server never answered.

use core::fmt;

use mcp_conformance_core::requirement::{RegistryError, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;

use crate::declared::{self, RevisionSource, UnjudgeableRevisions};
use crate::engine;
use crate::reader::{self, Limits, TraceParseError};
use crate::report::{Report, Verdict};

/// Why a document could not be judged.
#[derive(Debug)]
#[non_exhaustive]
pub enum JudgeError {
    /// The document is not a valid trace.
    Malformed(TraceParseError),
    /// The trace declares only revisions this build has no registry for.
    UnsupportedRevision(UnjudgeableRevisions),
    /// The trace judged no requirement at all — an empty or contentless
    /// recording, which is a capture that failed rather than a session that
    /// conformed.
    NothingJudged,
    /// The client spoke and the server never answered — see
    /// [`sessions::never_answered`](crate::sessions::never_answered): a
    /// capture that failed, not a session to judge.
    NeverAnswered(crate::sessions::NeverAnswered),
    /// A built-in registry failed to load (a build defect).
    Registry(RegistryError),
    /// [`Options::revisions`] names a revision this build has no registry for.
    UnknownRevision(ProtocolRevision),
}

impl fmt::Display for JudgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(error) => write!(f, "malformed trace: {error}"),
            Self::UnsupportedRevision(error) => error.fmt(f),
            Self::NothingJudged => f.write_str(
                "the trace judged no requirement at all — an empty or contentless trace is a \
                 capture that failed, not a session that conformed",
            ),
            Self::NeverAnswered(_) => f.write_str(
                "the server sent no message: the session never started, so the trace is a \
                 capture that failed, not a session that conformed",
            ),
            Self::Registry(error) => write!(f, "built-in registry: {error}"),
            Self::UnknownRevision(revision) => {
                write!(f, "this build has no registry for revision {revision}")
            }
        }
    }
}

impl std::error::Error for JudgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(error) => Some(error),
            Self::UnsupportedRevision(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::NothingJudged | Self::NeverAnswered(_) | Self::UnknownRevision(_) => None,
        }
    }
}

/// A judged trace: one report per revision it was judged at (one, unless it
/// declares several), each with how its revision was chosen.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Judgment {
    /// The reports, in ascending revision order.
    pub reports: Vec<Report>,
    /// The trace's events, in document order: what a
    /// [`sarif::Artifact`](crate::sarif::Artifact) needs to put each finding
    /// on the line holding its event.
    pub events: Vec<TraceEvent>,
}

impl Judgment {
    /// The most severe verdict across the reports: unsupported over fail over
    /// warnings over pass.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        Verdict::most_severe(self.reports.iter().map(Report::verdict)).unwrap_or(Verdict::Pass)
    }
}

/// How [`judge_with`] reads and judges a document: the reader's limits and
/// which revisions to judge at. The default is what [`judge`] does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// The reader's resource limits (`--max-events`, `--max-line-bytes`).
    pub limits: Limits,
    /// The revisions to judge at instead of those the trace declares
    /// (`--revision`), judged in ascending order and each once. Empty, the
    /// default, judges at the revisions the trace declares, else the newest.
    pub revisions: Vec<ProtocolRevision>,
}

impl Options {
    /// These options, reading under `limits`.
    #[must_use]
    pub const fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// These options, judging at `revisions` whatever the trace declares.
    #[must_use]
    pub fn revisions(mut self, revisions: impl IntoIterator<Item = ProtocolRevision>) -> Self {
        self.revisions = revisions.into_iter().collect();
        self
    }
}

/// Parses a JSON Lines trace document and judges it against the built-in
/// registries at the revision(s) it declares, as `mcp-trace-validator validate`
/// does. [`judge_with`] with the default [`Options`].
///
/// ```
/// use mcp_trace_validator::judge::{judge, JudgeError};
/// use mcp_trace_validator::report::Verdict;
///
/// let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
/// {"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
/// {"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;
/// let judgment = judge(trace)?;
/// assert_eq!(judgment.reports[0].revision, "2025-11-25");
/// assert_eq!(judgment.verdict(), Verdict::Pass);
///
/// // An empty recording is refused, not passed.
/// assert!(matches!(judge(""), Err(JudgeError::NothingJudged)));
///
/// // So is an initialize nothing answered: the server never took part.
/// let unanswered = trace.lines().next().unwrap_or_default();
/// assert!(matches!(judge(unanswered), Err(JudgeError::NeverAnswered(_))));
/// # Ok::<(), JudgeError>(())
/// ```
///
/// # Errors
///
/// [`JudgeError`] when the document is malformed, records a session the server
/// never answered, declares only unsupported revisions, or judges nothing.
pub fn judge(document: &str) -> Result<Judgment, JudgeError> {
    judge_with(document, &Options::default())
}

/// Parses and judges a JSON Lines trace document as [`judge`] does, under the
/// limits and at the revisions `options` set.
///
/// Those are the command line's `--max-events`, `--max-line-bytes` and
/// `--revision`.
///
/// ```
/// use mcp_trace_validator::judge::{judge_with, JudgeError, Options};
/// use mcp_trace_validator::reader::Limits;
///
/// // A 2025-06-18 session: no registry describes it, so it is refused…
/// let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
/// {"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;
/// assert!(matches!(
///     judge_with(trace, &Options::default()),
///     Err(JudgeError::UnsupportedRevision(_))
/// ));
///
/// // …unless the caller names the rules to judge it by.
/// let options = Options::default().revisions(["2025-11-25".parse()?]);
/// let judgment = judge_with(trace, &options)?;
/// assert_eq!(judgment.reports[0].revision, "2025-11-25");
/// assert_eq!(judgment.events.len(), 2);
///
/// // The reader's limits are the caller's too.
/// let small = Options::default().limits(Limits::new(1, 4096));
/// assert!(matches!(judge_with(trace, &small), Err(JudgeError::Malformed(_))));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
///
/// # Errors
///
/// As [`judge`], and [`JudgeError::UnknownRevision`] when
/// [`Options::revisions`] names one this build has no registry for.
pub fn judge_with(document: &str, options: &Options) -> Result<Judgment, JudgeError> {
    let events = reader::parse_trace(document, &options.limits).map_err(JudgeError::Malformed)?;
    if let Some(never) = crate::sessions::never_answered(&events) {
        return Err(JudgeError::NeverAnswered(never));
    }
    let set = RegistrySet::builtin().map_err(JudgeError::Registry)?;
    let (revisions, source) = if options.revisions.is_empty() {
        let selection =
            declared::select(set.revisions(), &events).map_err(JudgeError::UnsupportedRevision)?;
        (selection.revisions, selection.source)
    } else {
        let mut requested = options.revisions.clone();
        requested.sort_unstable();
        requested.dedup();
        if let Some(unknown) = requested
            .iter()
            .find(|revision| !set.revisions().contains(revision))
        {
            return Err(JudgeError::UnknownRevision(*unknown));
        }
        (requested, RevisionSource::Requested)
    };
    let reports: Vec<Report> = revisions
        .iter()
        .filter_map(|revision| set.registry(*revision))
        .map(|registry| {
            let mut report = engine::validate(&registry, &events);
            report.revision_source = Some(source);
            report
        })
        .collect();
    if reports.iter().all(|report| report.totals.judged_nothing()) {
        return Err(JudgeError::NothingJudged);
    }
    Ok(Judgment { reports, events })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::error::Error as _;

    use super::{JudgeError, Options, judge, judge_with};
    use crate::declared::RevisionSource;
    use crate::reader::{Limits, TraceParseError};

    const INITIALIZE_2025_06_18: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;

    #[test]
    fn each_refusal_says_why_and_keeps_its_cause() {
        let malformed = judge("not json").unwrap_err();
        assert!(matches!(malformed, JudgeError::Malformed(_)));
        assert!(
            malformed.to_string().starts_with("malformed trace: "),
            "{malformed}"
        );
        assert!(malformed.source().is_some());

        let unsupported = judge(INITIALIZE_2025_06_18).unwrap_err();
        assert!(matches!(unsupported, JudgeError::UnsupportedRevision(_)));
        assert!(
            unsupported.to_string().contains("2025-06-18"),
            "{unsupported}"
        );
        assert!(unsupported.source().is_some());

        let empty = judge("").unwrap_err();
        assert!(
            empty.to_string().contains("judged no requirement"),
            "{empty}"
        );
        assert!(empty.source().is_none());

        let unanswered = judge(INITIALIZE_2025_06_18.lines().next().unwrap()).unwrap_err();
        assert!(matches!(unanswered, JudgeError::NeverAnswered(_)));
        assert!(
            unanswered
                .to_string()
                .contains("the server sent no message"),
            "{unanswered}"
        );
        assert!(unanswered.source().is_none());
    }

    #[test]
    fn options_choose_the_revisions_each_once_in_order() {
        let both = Options::default().revisions(
            ["2026-07-28", "2025-11-25", "2026-07-28"].map(|revision| revision.parse().unwrap()),
        );
        let judgment = judge_with(INITIALIZE_2025_06_18, &both).unwrap();
        let revisions: Vec<&str> = judgment
            .reports
            .iter()
            .map(|report| report.revision.as_str())
            .collect();
        assert_eq!(revisions, ["2025-11-25", "2026-07-28"]);
        assert!(
            judgment
                .reports
                .iter()
                .all(|report| report.revision_source == Some(RevisionSource::Requested))
        );
        assert_eq!(judgment.events.len(), 2);

        let unknown = Options::default().revisions(["2025-06-18".parse().unwrap()]);
        let error = judge_with(INITIALIZE_2025_06_18, &unknown).unwrap_err();
        assert!(
            matches!(&error, JudgeError::UnknownRevision(revision) if revision.to_string() == "2025-06-18"),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            "this build has no registry for revision 2025-06-18"
        );
        assert!(error.source().is_none());
    }

    #[test]
    fn options_set_the_readers_limits() {
        let one_event = Options::default().limits(Limits::new(1, 4096));
        assert!(matches!(
            judge_with(INITIALIZE_2025_06_18, &one_event),
            Err(JudgeError::Malformed(TraceParseError::TooManyEvents {
                limit: 1
            }))
        ));
        // The default limits are the reader's.
        assert_eq!(Options::default().limits, Limits::default());
        assert!(Options::default().revisions.is_empty());
    }
}
