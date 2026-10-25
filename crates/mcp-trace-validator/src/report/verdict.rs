// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The verdict of a validation run, and how verdicts rank.

use core::fmt;

use serde::{Deserialize, Serialize};

/// Overall verdict of a validation run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Verdict {
    /// No findings at all.
    Pass,
    /// Only SHOULD-level findings.
    PassWithWarnings,
    /// At least one MUST-level violation, or a message invalid against the
    /// revision's JSON Schema.
    Fail,
    /// The registry and this build disagree about available checks.
    Unsupported,
    /// A run over several traces could not judge at least one of them — it was
    /// unreadable, malformed, of an unsupported revision, or judged nothing —
    /// so no verdict covers the whole run. Never the verdict of one report: a
    /// trace that cannot be judged has no report.
    ///
    /// Ranked above every other verdict, as its exit status (`2` or `3`) ranks
    /// above theirs: whatever the judged traces say, a run that left one out
    /// must not read as passing.
    Incomplete,
}

impl Verdict {
    /// The most severe of `verdicts` — incomplete over unsupported over fail
    /// over warnings over pass, the order a report derives its own — or `None`
    /// when there are none.
    pub fn most_severe(verdicts: impl IntoIterator<Item = Self>) -> Option<Self> {
        verdicts
            .into_iter()
            .max_by_key(|verdict| verdict.severity())
    }

    const fn severity(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::PassWithWarnings => 1,
            Self::Fail => 2,
            Self::Unsupported => 3,
            Self::Incomplete => 4,
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Pass => "pass",
            Self::PassWithWarnings => "pass-with-warnings",
            Self::Fail => "fail",
            Self::Unsupported => "unsupported",
            Self::Incomplete => "incomplete",
        };
        f.write_str(text)
    }
}
