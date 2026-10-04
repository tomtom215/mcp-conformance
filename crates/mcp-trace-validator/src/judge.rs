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

use crate::declared::{self, UnjudgeableRevisions};
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
        }
    }
}

impl std::error::Error for JudgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(error) => Some(error),
            Self::UnsupportedRevision(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::NothingJudged | Self::NeverAnswered(_) => None,
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
}

impl Judgment {
    /// The most severe verdict across the reports: unsupported over fail over
    /// warnings over pass.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        let rank = |verdict: &Verdict| match verdict {
            Verdict::Pass => 0,
            Verdict::PassWithWarnings => 1,
            Verdict::Fail => 2,
            _ => 3,
        };
        self.reports
            .iter()
            .map(Report::verdict)
            .max_by_key(rank)
            .unwrap_or(Verdict::Pass)
    }
}

/// Parses a JSON Lines trace document and judges it against the built-in
/// registries at the revision(s) it declares, as `mcp-trace-validator validate`
/// does.
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
    let events =
        reader::parse_trace(document, &Limits::default()).map_err(JudgeError::Malformed)?;
    if let Some(never) = crate::sessions::never_answered(&events) {
        return Err(JudgeError::NeverAnswered(never));
    }
    let set = RegistrySet::builtin().map_err(JudgeError::Registry)?;
    let selection =
        declared::select(set.revisions(), &events).map_err(JudgeError::UnsupportedRevision)?;
    let reports: Vec<Report> = selection
        .revisions
        .iter()
        .filter_map(|revision| set.registry(*revision))
        .map(|registry| {
            let mut report = engine::validate(&registry, &events);
            report.revision_source = Some(selection.source);
            report
        })
        .collect();
    if reports.iter().all(|report| report.totals.judged_nothing()) {
        return Err(JudgeError::NothingJudged);
    }
    Ok(Judgment { reports })
}
