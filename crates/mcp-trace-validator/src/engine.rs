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
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::report::{ClauseSource, Outcome, Report, RequirementReport, Totals};

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
///
/// # Panics
///
/// When `events` is not in strictly increasing `seq` order — a contract
/// violation only a hand-built slice can commit, since
/// [`reader::parse_trace`](crate::reader::parse_trace) rejects such
/// documents ([`TraceContext::new`] documents the reasoning).
#[must_use]
pub fn validate(registry: &Registry, events: &[TraceEvent]) -> Report {
    validate_ordered(registry, events)
}

/// [`validate`] for events that did not come from
/// [`reader::parse_trace`](crate::reader::parse_trace): the same report, or an
/// error naming the first pair of events out of `seq` order instead of a panic.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, reader};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let line = |seq| format!(r#"{{"seq":{seq},"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}}"#);
/// let mut events = reader::parse_trace(&format!("{}\n{}", line(0), line(1)), &reader::Limits::default())?;
/// events.swap(0, 1);
/// let error = engine::try_validate(&registry, &events).unwrap_err();
/// assert_eq!((error.index, error.seq, error.previous), (1, 0, 1));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
///
/// # Errors
///
/// [`SeqOrderError`] when `seq` is not strictly increasing across `events`.
pub fn try_validate(registry: &Registry, events: &[TraceEvent]) -> Result<Report, SeqOrderError> {
    events
        .windows(2)
        .position(|pair| pair[0].seq >= pair[1].seq)
        .map_or_else(
            || Ok(validate_ordered(registry, events)),
            |position| {
                Err(SeqOrderError {
                    index: position + 1,
                    seq: events[position + 1].seq,
                    previous: events[position].seq,
                })
            },
        )
}

/// Events out of `seq` order: the trace format requires `seq` strictly
/// increasing, and several checks rely on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SeqOrderError {
    /// Index in the slice of the first event whose `seq` is not greater than its
    /// predecessor's.
    pub index: usize,
    /// That event's `seq`.
    pub seq: u64,
    /// The preceding event's `seq`.
    pub previous: u64,
}

impl core::fmt::Display for SeqOrderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "event {} has seq {}, not greater than the preceding seq {}; trace events \
             must have strictly increasing seq",
            self.index, self.seq, self.previous
        )
    }
}

impl core::error::Error for SeqOrderError {}

fn validate_ordered(registry: &Registry, events: &[TraceEvent]) -> Report {
    let context = TraceContext::new(events);
    let mut totals = Totals::default();
    let mut rows = Vec::with_capacity(registry.requirements().len());

    for requirement in registry.requirements() {
        let row = build_row(requirement, registry.revision(), &context);
        tally(&mut totals, row.outcome);
        rows.push(row);
    }

    Report {
        revision: registry.revision().to_string(),
        revision_mismatch: crate::declared::mismatch(registry.revision(), events),
        revision_source: None,
        totals,
        requirements: rows,
    }
}

fn build_row(
    requirement: &Requirement,
    revision: ProtocolRevision,
    context: &TraceContext<'_>,
) -> RequirementReport {
    let mut row = RequirementReport {
        id: requirement.id.to_string(),
        level: requirement.level.keyword().to_owned(),
        outcome: Outcome::Unsupported,
        findings: vec![],
        exclusion: None,
        missing_checks: vec![],
        capability: None,
        source: None,
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
                // One check finding something to judge is enough: requirements
                // sharing several checks are observed if any of them had a
                // subject, and only a requirement none of them could bind to
                // reports "not observed".
                let mut observed = false;
                for check in resolved {
                    let outcome = check.run(context);
                    observed |= outcome.subjects > 0;
                    row.findings.extend(outcome.findings);
                }
                row.outcome = if row.findings.is_empty() && !observed {
                    Outcome::NotObserved
                } else {
                    classify_outcome(requirement.level.is_error(), row.findings.is_empty())
                };
            }
        }
        // Verification is #[non_exhaustive]; a future arm must be handled
        // deliberately, and the pre-set "unsupported" outcome is the conservative
        // reading until then.
        _ => {}
    }
    if matches!(row.outcome, Outcome::Fail | Outcome::Warn) {
        row.source = Some(ClauseSource::new(requirement, revision));
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
        Outcome::NotObserved => totals.not_observed += 1,
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
mod tests;
