// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Validation reports: per-requirement outcomes with actionable findings.
//!
//! Reports are artifacts: they get committed as golden files, diffed in CI, and cited
//! in published results. Two consequences shape this module: serialization order is
//! fixed (registry order; struct fields in declaration order), and nothing
//! environment-dependent (paths, timestamps, hostnames) is ever included.

use core::fmt;
use core::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// One concrete violation, addressed to a requirement and (where possible) an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Finding {
    /// The validator check that produced this finding.
    pub check: String,
    /// The event `seq` the finding points at, when one event is identifiable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// What was observed and what was expected, in one actionable sentence.
    pub detail: String,
}

/// The outcome of evaluating one requirement against one trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Outcome {
    /// All covering checks ran and produced no findings.
    Pass,
    /// A MUST / MUST NOT requirement has findings.
    Fail,
    /// A SHOULD / SHOULD NOT requirement has findings.
    Warn,
    /// The registry documents that this requirement is not judged from traces.
    Excluded,
    /// The registry references a check this validator build does not implement.
    Unsupported,
    /// The requirement is gated on a capability this session never declared
    /// (ADR-0006); its checks were not run.
    NotApplicable,
}

/// Aggregate counts, in report order. `excluded` and `unsupported` are first-class:
/// inflating pass rates by hiding them is how conformance tools lose trust.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Totals {
    /// Requirements with outcome [`Outcome::Pass`].
    pub pass: u32,
    /// Requirements with outcome [`Outcome::Fail`].
    pub fail: u32,
    /// Requirements with outcome [`Outcome::Warn`].
    pub warn: u32,
    /// Requirements with outcome [`Outcome::Excluded`].
    pub excluded: u32,
    /// Requirements with outcome [`Outcome::Unsupported`].
    pub unsupported: u32,
    /// Requirements with outcome [`Outcome::NotApplicable`].
    pub not_applicable: u32,
}

/// One requirement's row in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RequirementReport {
    /// The requirement ID (`AREA-NNN`).
    pub id: String,
    /// The requirement's RFC 2119 level, as registry text (`"MUST"`, …).
    pub level: String,
    /// The evaluation outcome.
    pub outcome: Outcome,
    /// Findings, in event order. Empty unless `outcome` is `fail` or `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
    /// The documented exclusion reason, when `outcome` is `excluded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion: Option<String>,
    /// Check IDs the build lacks, when `outcome` is `unsupported`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_checks: Vec<String>,
    /// The undeclared capability gate, when `outcome` is `not-applicable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
}

/// A complete validation report for one trace against one registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Report {
    /// The registry's protocol revision (`YYYY-MM-DD`).
    pub revision: String,
    /// Aggregate counts.
    pub totals: Totals,
    /// Per-requirement outcomes, in registry order.
    pub requirements: Vec<RequirementReport>,
}

impl Report {
    /// `true` when any requirement failed (errors, not warnings).
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        self.totals.fail > 0
    }

    /// `true` when any SHOULD-level requirement produced findings.
    #[must_use]
    pub const fn has_warnings(&self) -> bool {
        self.totals.warn > 0
    }

    /// `true` when the registry referenced checks this build does not implement.
    #[must_use]
    pub const fn has_unsupported(&self) -> bool {
        self.totals.unsupported > 0
    }

    /// Renders the human-readable form.
    #[must_use]
    pub fn render_human(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "MCP trace validation — revision {}", self.revision);
        for row in &self.requirements {
            let marker = match row.outcome {
                Outcome::Pass => "PASS",
                Outcome::Fail => "FAIL",
                Outcome::Warn => "WARN",
                Outcome::Excluded => "EXCL",
                Outcome::Unsupported => "UNSUP",
                Outcome::NotApplicable => "N/A",
            };
            let _ = writeln!(out, "  {marker:<5} {} ({})", row.id, row.level);
            for finding in &row.findings {
                match finding.seq {
                    Some(seq) => {
                        let _ = writeln!(out, "        seq {seq}: {}", finding.detail);
                    }
                    None => {
                        let _ = writeln!(out, "        {}", finding.detail);
                    }
                }
            }
            if let Some(exclusion) = &row.exclusion {
                let _ = writeln!(out, "        excluded: {exclusion}");
            }
            for check in &row.missing_checks {
                let _ = writeln!(out, "        unsupported check: {check}");
            }
            if let Some(capability) = &row.capability {
                let _ = writeln!(
                    out,
                    "        not applicable: capability {capability} was not declared in this session"
                );
            }
        }
        let totals = self.totals;
        let _ = writeln!(
            out,
            "totals: {} pass, {} fail, {} warn, {} excluded, {} unsupported, {} not applicable",
            totals.pass,
            totals.fail,
            totals.warn,
            totals.excluded,
            totals.unsupported,
            totals.not_applicable
        );
        let _ = writeln!(out, "verdict: {}", self.verdict());
        out
    }

    /// One-word verdict for the trailing summary line.
    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        if self.totals.unsupported > 0 {
            Verdict::Unsupported
        } else if self.totals.fail > 0 {
            Verdict::Fail
        } else if self.totals.warn > 0 {
            Verdict::PassWithWarnings
        } else {
            Verdict::Pass
        }
    }
}

/// Overall verdict of a validation run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Verdict {
    /// No findings at all.
    Pass,
    /// Only SHOULD-level findings.
    PassWithWarnings,
    /// At least one MUST-level violation.
    Fail,
    /// The registry and this build disagree about available checks.
    Unsupported,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Pass => "pass",
            Self::PassWithWarnings => "pass-with-warnings",
            Self::Fail => "fail",
            Self::Unsupported => "unsupported",
        };
        f.write_str(text)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample() -> Report {
        Report {
            revision: "2025-11-25".to_owned(),
            totals: Totals {
                pass: 1,
                fail: 1,
                warn: 0,
                excluded: 1,
                unsupported: 0,
                not_applicable: 1,
            },
            requirements: vec![
                RequirementReport {
                    id: "BASE-001".to_owned(),
                    level: "MUST".to_owned(),
                    outcome: Outcome::Pass,
                    findings: vec![],
                    exclusion: None,
                    missing_checks: vec![],
                    capability: None,
                },
                RequirementReport {
                    id: "LIFE-001".to_owned(),
                    level: "MUST".to_owned(),
                    outcome: Outcome::Fail,
                    findings: vec![Finding {
                        check: "lifecycle.first-interaction-initialize".to_owned(),
                        seq: Some(3),
                        detail: "first message is \"tools/list\", expected \"initialize\""
                            .to_owned(),
                    }],
                    exclusion: None,
                    missing_checks: vec![],
                    capability: None,
                },
                RequirementReport {
                    id: "TRAN-001".to_owned(),
                    level: "MUST NOT".to_owned(),
                    outcome: Outcome::Excluded,
                    findings: vec![],
                    exclusion: Some("enforced at capture time".to_owned()),
                    missing_checks: vec![],
                    capability: None,
                },
                RequirementReport {
                    id: "TOOL-001".to_owned(),
                    level: "MUST".to_owned(),
                    outcome: Outcome::NotApplicable,
                    findings: vec![],
                    exclusion: None,
                    missing_checks: vec![],
                    capability: Some("server.tools".to_owned()),
                },
            ],
        }
    }

    #[test]
    fn verdict_priority_is_unsupported_fail_warn_pass() {
        let mut report = sample();
        assert_eq!(report.verdict(), Verdict::Fail);
        report.totals.unsupported = 1;
        assert_eq!(report.verdict(), Verdict::Unsupported);
        report.totals.unsupported = 0;
        report.totals.fail = 0;
        report.totals.warn = 2;
        assert_eq!(report.verdict(), Verdict::PassWithWarnings);
        report.totals.warn = 0;
        assert_eq!(report.verdict(), Verdict::Pass);
    }

    #[test]
    fn human_rendering_shows_findings_and_totals() {
        let text = sample().render_human();
        assert!(text.contains("FAIL  LIFE-001 (MUST)"), "{text}");
        assert!(text.contains("seq 3:"), "{text}");
        assert!(
            text.contains("excluded: enforced at capture time"),
            "{text}"
        );
        assert!(text.contains("N/A   TOOL-001 (MUST)"), "{text}");
        assert!(
            text.contains(
                "not applicable: capability server.tools was not declared in this session"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "totals: 1 pass, 1 fail, 0 warn, 1 excluded, 0 unsupported, 1 not applicable"
            ),
            "{text}"
        );
        assert!(text.contains("verdict: fail"), "{text}");
    }

    #[test]
    fn json_omits_empty_collections() {
        let report = sample();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"revision\":\"2025-11-25\""), "{json}");
        // Passing rows carry no findings/exclusion/missing_checks keys.
        assert!(!json.contains("\"missing_checks\""), "{json}");
    }

    #[test]
    fn totals_predicates_pin_their_thresholds() {
        let mut report = sample();
        report.totals = Totals::default();
        assert!(!report.has_errors());
        assert!(!report.has_warnings());
        assert!(!report.has_unsupported());
        report.totals.fail = 1;
        assert!(report.has_errors());
        report.totals.warn = 1;
        assert!(report.has_warnings());
        report.totals.unsupported = 1;
        assert!(report.has_unsupported());
    }
}
