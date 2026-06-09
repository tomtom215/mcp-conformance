// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The validation engine: registry × trace → report.
//!
//! [`validate`] is a pure function. It builds the [`TraceContext`] once, then walks the
//! registry **in registry order**, producing exactly one [`RequirementReport`] per
//! requirement. Checks shared across requirements run once per referencing requirement —
//! the same evidence violating two clauses is two findings, which is what
//! requirement-level accounting means.

use mcp_conformance_core::capability::{CapabilityGate, CapabilityParty};
use mcp_conformance_core::requirement::{Registry, Requirement, Verification};
use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::report::{Outcome, Report, RequirementReport, Totals};

/// Validates a parsed trace against a requirement registry.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::report::Verdict;
/// use mcp_trace_validator::{engine, reader};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;
/// let events = reader::parse_trace(trace, &reader::Limits::default())?;
/// let report = engine::validate(&registry, &events);
/// assert_eq!(report.verdict(), Verdict::Fail); // tools/list before initialize
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
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
        capability: None,
    };
    match &requirement.verification {
        Verification::Excluded { exclusion } => {
            row.outcome = Outcome::Excluded;
            row.exclusion = Some(exclusion.clone());
        }
        Verification::Checks { checks: check_ids } => {
            // Resolve the inventory before consulting the capability gate:
            // `unsupported` is a property of (registry, build) and must not vary
            // with what a particular trace negotiated (ADR-0006 precedence).
            let mut resolved = Vec::with_capacity(check_ids.len());
            for check_id in check_ids {
                match checks::find(check_id) {
                    Some(check) => resolved.push(check),
                    None => row.missing_checks.push(check_id.clone()),
                }
            }
            if !row.missing_checks.is_empty() {
                row.outcome = Outcome::Unsupported;
            } else if let Some(gate) = undeclared_gate(requirement, context) {
                row.outcome = Outcome::NotApplicable;
                row.capability = Some(gate.as_str().to_owned());
            } else {
                for check in resolved {
                    row.findings.extend(check.run(context));
                }
                row.outcome =
                    classify_outcome(requirement.level.is_error(), row.findings.is_empty());
            }
        }
        // Verification is #[non_exhaustive]; a future arm must be handled
        // deliberately, and the pre-set "unsupported" outcome is the conservative
        // reading until then.
        _ => {}
    }
    row
}

/// The requirement's capability gate, when the session never declared it.
fn undeclared_gate<'r>(
    requirement: &'r Requirement,
    context: &TraceContext<'_>,
) -> Option<&'r CapabilityGate> {
    let gate = requirement.capability.as_ref()?;
    let capabilities = match gate.party() {
        CapabilityParty::Server => context.server_capabilities(),
        CapabilityParty::Client => context.client_capabilities(),
    };
    if gate.is_declared(capabilities) {
        None
    } else {
        Some(gate)
    }
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
        Outcome::NotApplicable => totals.not_applicable += 1,
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
        use mcp_conformance_core::requirement::Verification;
        let registry = Registry::builtin_2025_11_25().unwrap();
        let events = parse_trace(HAPPY, &Limits::default()).unwrap();
        let report = validate(&registry, &events);
        assert!(!report.has_errors(), "{}", report.render_human());
        assert!(!report.has_warnings(), "{}", report.render_human());
        let documented_exclusions = registry
            .requirements()
            .iter()
            .filter(|requirement| matches!(requirement.verification, Verification::Excluded { .. }))
            .count();
        assert_eq!(
            usize::try_from(report.totals.excluded).unwrap(),
            documented_exclusions,
            "every documented exclusion reports as excluded, regardless of trace"
        );
        // This handshake declares no capabilities, so every gated requirement
        // must surface as not-applicable — never as a vacuous pass.
        let gated = registry
            .requirements()
            .iter()
            .filter(|requirement| {
                requirement.capability.is_some()
                    && matches!(requirement.verification, Verification::Checks { .. })
            })
            .count();
        assert_eq!(
            usize::try_from(report.totals.not_applicable).unwrap(),
            gated,
            "{}",
            report.render_human()
        );
        assert_eq!(report.totals.unsupported, 0);
        assert_eq!(
            usize::try_from(
                report.totals.pass
                    + report.totals.fail
                    + report.totals.warn
                    + report.totals.excluded
                    + report.totals.unsupported
                    + report.totals.not_applicable
            )
            .unwrap(),
            registry.requirements().len(),
            "every requirement is accounted for exactly once"
        );
    }

    /// One-requirement registry gated on `server.tools`, with a real check.
    const GATED_REGISTRY: &str = r#"{
        "revision": "2025-11-25",
        "requirements": [
            {"id": "TOOL-001", "level": "MUST", "actor": "server",
             "capability": "server.tools",
             "source": {"section": "server/tools#x", "quote": "MUST t"},
             "checks": ["base.jsonrpc-version"]}
        ]
    }"#;

    fn handshake(server_capabilities: &str) -> String {
        format!(
            r#"{{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"t","version":"0"}}}}}}}}
{{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{server_capabilities},"serverInfo":{{"name":"s","version":"0"}}}}}}}}
{{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","method":"notifications/initialized"}}}}"#
        )
    }

    #[test]
    fn undeclared_capability_reports_not_applicable_not_pass() {
        let registry = Registry::from_json(GATED_REGISTRY).unwrap();
        let trace = handshake(r#"{"prompts":{}}"#);
        let events = parse_trace(&trace, &Limits::default()).unwrap();
        let report = validate(&registry, &events);
        assert_eq!(report.totals.not_applicable, 1);
        assert_eq!(report.totals.pass, 0);
        assert_eq!(
            report.requirements[0].outcome,
            crate::report::Outcome::NotApplicable
        );
        assert_eq!(
            report.requirements[0].capability.as_deref(),
            Some("server.tools")
        );
        assert_eq!(report.verdict(), crate::report::Verdict::Pass);
    }

    #[test]
    fn declared_capability_runs_the_gated_checks() {
        let registry = Registry::from_json(GATED_REGISTRY).unwrap();
        let trace = handshake(r#"{"tools":{"listChanged":true}}"#);
        let events = parse_trace(&trace, &Limits::default()).unwrap();
        let report = validate(&registry, &events);
        assert_eq!(report.totals.not_applicable, 0);
        assert_eq!(report.totals.pass, 1);
        assert!(report.requirements[0].capability.is_none());
    }

    #[test]
    fn missing_checks_outrank_the_capability_gate() {
        // `unsupported` must be a property of (registry, build), not of what one
        // trace negotiated — a gated requirement with an unknown check is
        // unsupported even when the capability was never declared.
        let registry_json = r#"{
            "revision": "2025-11-25",
            "requirements": [
                {"id": "TOOL-001", "level": "MUST", "actor": "server",
                 "capability": "server.tools",
                 "source": {"section": "server/tools#x", "quote": "MUST t"},
                 "checks": ["future.not-built-yet"]}
            ]
        }"#;
        let registry = Registry::from_json(registry_json).unwrap();
        let report = validate(&registry, &[]);
        assert_eq!(report.totals.unsupported, 1);
        assert_eq!(report.totals.not_applicable, 0);
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
