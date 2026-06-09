// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The validation engine: registry × trace → report.
//!
//! [`validate`] is a pure function. It builds the [`TraceContext`] once, then walks the
//! registry **in registry order**, producing exactly one [`RequirementReport`] per
//! requirement. Checks shared across requirements run once per referencing requirement —
//! the same evidence violating two clauses is two findings, which is what
//! requirement-level accounting means.

use mcp_conformance_core::requirement::{Registry, Requirement, Verification};
use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::report::{Outcome, Report, RequirementReport, Totals};

/// Validates a parsed trace against a requirement registry.
#[must_use]
pub fn validate(registry: &Registry, events: &[TraceEvent]) -> Report {
    let context = TraceContext::new(events);
    let mut totals = Totals::default();
    let mut rows = Vec::with_capacity(registry.requirements().len());

    for requirement in registry.requirements() {
        let row = build_row(requirement, &context);
        tally(&mut totals, row.outcome);
        rows.push(row);
    }

    Report {
        revision: registry.revision().to_string(),
        totals,
        requirements: rows,
    }
}

fn build_row(requirement: &Requirement, context: &TraceContext<'_>) -> RequirementReport {
    let mut row = RequirementReport {
        id: requirement.id.to_string(),
        level: requirement.level.keyword().to_owned(),
        outcome: Outcome::Unsupported,
        findings: vec![],
        exclusion: None,
        missing_checks: vec![],
    };
    match &requirement.verification {
        Verification::Excluded { exclusion } => {
            row.outcome = Outcome::Excluded;
            row.exclusion = Some(exclusion.clone());
        }
        Verification::Checks { checks: check_ids } => {
            for check_id in check_ids {
                match checks::find(check_id) {
                    Some(check) => row.findings.extend(check.run(context)),
                    None => row.missing_checks.push(check_id.clone()),
                }
            }
            row.outcome = if row.missing_checks.is_empty() {
                classify_outcome(requirement.level.is_error(), row.findings.is_empty())
            } else {
                Outcome::Unsupported
            };
        }
        // Verification is #[non_exhaustive]; a future arm must be handled
        // deliberately, and the pre-set "unsupported" outcome is the conservative
        // reading until then.
        _ => {}
    }
    row
}

/// Exhaustive on purpose (same-crate enum): adding an Outcome variant must force a
/// deliberate decision about how totals count it.
const fn tally(totals: &mut Totals, outcome: Outcome) {
    match outcome {
        Outcome::Pass => totals.pass += 1,
        Outcome::Fail => totals.fail += 1,
        Outcome::Warn => totals.warn += 1,
        Outcome::Excluded => totals.excluded += 1,
        Outcome::Unsupported => totals.unsupported += 1,
    }
}

const fn classify_outcome(is_error_level: bool, clean: bool) -> Outcome {
    if clean {
        Outcome::Pass
    } else if is_error_level {
        Outcome::Fail
    } else {
        Outcome::Warn
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::reader::{Limits, parse_trace};
    use mcp_conformance_core::requirement::Registry;

    const HAPPY: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}
{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;

    #[test]
    fn happy_path_passes_every_checked_requirement() {
        let registry = Registry::builtin_2025_11_25().unwrap();
        let events = parse_trace(HAPPY, &Limits::default()).unwrap();
        let report = validate(&registry, &events);
        assert!(!report.has_errors(), "{}", report.render_human());
        assert!(!report.has_warnings(), "{}", report.render_human());
        assert_eq!(
            report.totals.excluded, 2,
            "TRAN-001 and TRAN-002 are excluded"
        );
        assert_eq!(report.totals.unsupported, 0);
        assert_eq!(
            usize::try_from(
                report.totals.pass
                    + report.totals.fail
                    + report.totals.warn
                    + report.totals.excluded
                    + report.totals.unsupported
            )
            .unwrap(),
            registry.requirements().len(),
            "every requirement is accounted for exactly once"
        );
    }

    #[test]
    fn unknown_check_reports_unsupported_not_silence() {
        let registry_json = r#"{
            "revision": "2025-11-25",
            "requirements": [
                {"id": "FUTR-001", "level": "MUST", "actor": "both",
                 "source": {"section": "future#x", "quote": "MUST do future things"},
                 "checks": ["future.not-built-yet"]}
            ]
        }"#;
        let registry = Registry::from_json(registry_json).unwrap();
        let report = validate(&registry, &[]);
        assert_eq!(report.totals.unsupported, 1);
        assert!(report.has_unsupported());
        assert_eq!(
            report.requirements[0].missing_checks,
            ["future.not-built-yet"]
        );
    }

    #[test]
    fn report_is_deterministic_across_runs() {
        let registry = Registry::builtin_2025_11_25().unwrap();
        let events = parse_trace(HAPPY, &Limits::default()).unwrap();
        let a = serde_json::to_string(&validate(&registry, &events)).unwrap();
        let b = serde_json::to_string(&validate(&registry, &events)).unwrap();
        assert_eq!(a, b);
    }
}
