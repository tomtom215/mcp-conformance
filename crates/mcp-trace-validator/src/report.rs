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
    /// Every covering check ran and found nothing to judge: this session
    /// carried none of the traffic the clause binds to.
    ///
    /// Distinct from [`Self::Pass`], and the distinction is the whole point. A
    /// clause about `subscriptions/listen` cannot be *complied with* by a
    /// session that never opened a stream — there was no opportunity to break
    /// it — so reporting `pass` states evidence the trace does not carry.
    /// Distinct from [`Self::NotApplicable`] too: that one is the registry
    /// saying the clause is gated on a capability nobody declared, this one is
    /// the trace saying it had nothing to show.
    NotObserved,
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
    /// Requirements with outcome [`Outcome::NotObserved`].
    pub not_observed: u32,
}

impl Totals {
    /// Every outcome's report label and count, in report order.
    ///
    /// Destructured exhaustively on purpose, and that is the whole point of the
    /// method existing: a field added to [`Totals`] fails to compile here until
    /// it is given a label, and every summary line in the crate is formatted
    /// from this one list. Hand-written `write!` arms could not offer that —
    /// the single-revision line named all seven outcomes while the
    /// multi-revision line named six, so the same run reported 140 clauses as
    /// human text and 140 as JSON but only accounted for 125 of them in the
    /// former. Counts that do not add up are how a conformance tool overstates
    /// what it judged.
    #[must_use]
    pub const fn labelled(&self) -> [(&'static str, u32); 7] {
        let Self {
            pass,
            fail,
            warn,
            excluded,
            unsupported,
            not_applicable,
            not_observed,
        } = *self;
        [
            ("pass", pass),
            ("fail", fail),
            ("warn", warn),
            ("excluded", excluded),
            ("unsupported", unsupported),
            ("not applicable", not_applicable),
            ("not observed", not_observed),
        ]
    }
}

/// The counts as one phrase — `23 pass, 0 fail, …` — naming every outcome.
///
/// The summary lines differ in what surrounds them (`totals: ` on a
/// single-revision report, the revision and its verdict on a multi-revision
/// one) and agree on what is inside, so what is inside is written once.
impl fmt::Display for Totals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (label, count)) in self.labelled().into_iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{count} {label}")?;
        }
        Ok(())
    }
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
                Outcome::NotObserved => "NOBS",
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
            if row.outcome == Outcome::NotObserved {
                let _ = writeln!(
                    out,
                    "        not observed: the session carried none of the traffic this clause binds to"
                );
            }
        }
        // Every outcome is named, so the counts sum to the registry's size — a
        // reader can check the arithmetic, and `Totals`' own exhaustive
        // destructuring is what keeps that true as outcomes are added.
        let _ = writeln!(out, "totals: {}", self.totals);
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

    fn row(id: &str, level: &str, outcome: Outcome) -> RequirementReport {
        RequirementReport {
            id: id.to_owned(),
            level: level.to_owned(),
            outcome,
            findings: vec![],
            exclusion: None,
            missing_checks: vec![],
            capability: None,
        }
    }

    /// One row of every outcome the renderer can produce, so the totals line
    /// and the per-row text are pinned against the full set rather than a
    /// convenient subset.
    fn sample() -> Report {
        let mut failed = row("LIFE-001", "MUST", Outcome::Fail);
        failed.findings = vec![Finding {
            check: "lifecycle.first-interaction-initialize".to_owned(),
            seq: Some(3),
            detail: "first message is \"tools/list\", expected \"initialize\"".to_owned(),
        }];
        let mut excluded = row("TRAN-001", "MUST NOT", Outcome::Excluded);
        excluded.exclusion = Some("enforced at capture time".to_owned());
        let mut not_applicable = row("TOOL-001", "MUST", Outcome::NotApplicable);
        not_applicable.capability = Some("server.tools".to_owned());
        Report {
            revision: "2025-11-25".to_owned(),
            totals: Totals {
                pass: 1,
                fail: 1,
                warn: 0,
                excluded: 1,
                unsupported: 0,
                not_applicable: 1,
                not_observed: 1,
            },
            requirements: vec![
                row("BASE-001", "MUST", Outcome::Pass),
                failed,
                excluded,
                not_applicable,
                row("PAGE-002", "MUST", Outcome::NotObserved),
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
        // A not-observed row says so in words, like every other non-judged
        // outcome: "NOBS" alone tells an operator nothing about *why*. Pinned
        // as the two lines *together*, and counted: asserting only that the
        // sentence appears somewhere passes just as well when it is attached
        // to every row except the one it describes.
        assert!(
            text.contains(
                "  NOBS  PAGE-002 (MUST)\n        not observed: the session carried none of \
                 the traffic this clause binds to\n"
            ),
            "{text}"
        );
        assert_eq!(
            text.matches("not observed:").count(),
            1,
            "exactly the not-observed row carries the reason: {text}"
        );
        // The whole line, anchored at both ends: a `contains` of a prefix would
        // pass while a new outcome went unnamed and the counts stopped summing
        // to the registry's size.
        assert!(
            text.contains(
                "\ntotals: 1 pass, 1 fail, 0 warn, 1 excluded, 0 unsupported, \
                 1 not applicable, 1 not observed\n"
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

    /// The counts a rendered summary line actually carries, read back out of
    /// the text a reader sees rather than off the struct the renderer was
    /// handed — the two disagreeing is the whole failure this guards.
    fn counts_in(line: &str) -> Vec<u32> {
        // The first number in each comma-separated part is its count; what
        // surrounds it (`totals: ` here, a revision there) carries none.
        line.split(", ")
            .filter_map(|part| part.split_whitespace().find_map(|word| word.parse().ok()))
            .collect()
    }

    #[test]
    fn a_summary_line_accounts_for_every_requirement() {
        let report = sample();
        let text = report.render_human();
        let line = text
            .lines()
            .find(|line| line.starts_with("totals: "))
            .unwrap();
        let counts = counts_in(line);
        assert_eq!(
            counts.len(),
            Totals::default().labelled().len(),
            "every outcome is named: {line}"
        );
        // The invariant the line's own comment claims, asserted rather than
        // left to a reader's arithmetic: what the renderer prints must add up
        // to the rows it printed. The multi-revision line made exactly this
        // claim in prose and silently broke it.
        assert_eq!(
            counts.iter().sum::<u32>() as usize,
            report.requirements.len(),
            "{line}"
        );
    }

    #[test]
    fn every_outcome_has_a_label_and_they_are_distinct() {
        let labels: Vec<&str> = Totals::default()
            .labelled()
            .iter()
            .map(|&(label, _)| label)
            .collect();
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), labels.len(), "duplicate label in {labels:?}");
        // Each count sits with its own label: a swapped pair would keep the sum
        // and the label set intact, so the mapping is pinned too.
        let totals = Totals {
            pass: 1,
            fail: 2,
            warn: 3,
            excluded: 4,
            unsupported: 5,
            not_applicable: 6,
            not_observed: 7,
        };
        assert_eq!(
            totals.to_string(),
            "1 pass, 2 fail, 3 warn, 4 excluded, 5 unsupported, 6 not applicable, 7 not observed"
        );
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
