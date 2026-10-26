// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The validation engine: registry × trace → report.
//!
//! [`validate`] is a pure function. It builds the [`TraceContext`] once, then walks the
//! registry **in registry order**, producing exactly one [`RequirementReport`] per
//! requirement. Checks shared across requirements run once per referencing requirement —
//! the same evidence violating two clauses is two findings, which is what
//! requirement-level accounting means.
//!
//! The registry's extension clauses are judged the same way and reported apart, one
//! [`ExtensionReport`] per extension, with totals of their own that the report's
//! totals — and so its verdict — never see (ADR-0023).

use mcp_conformance_core::capability::{CapabilityGate, CapabilityParty};
use mcp_conformance_core::requirement::{Actor, Registry, Requirement, Verification};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::report::{
    ClauseSource, ExtensionReport, Finding, Outcome, Party, Report, RequirementReport, Totals,
};

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
        party_view: None,
        totals,
        requirements: rows,
        wire_schema: crate::wire_schema::check(registry.revision(), &context),
        unanswered_requests: crate::unanswered::find(&context),
        extensions: extension_reports(registry, &context),
    }
}

/// Holds each of `findings` against a party: the clause's actor when it binds
/// one party, else the sender of the message at the finding's `seq`.
///
/// For a clause binding both parties the event decides, because every such
/// check points its finding at the offending message itself — the request
/// with the reused id, the result with no `result`, the `logging/setLevel`
/// sent to a server that never declared `logging` — so its sender is the one
/// who broke the clause. A clause binding one party is that party's whatever
/// the event: `LIFE-003` points at the server's `initialize` result, and the
/// omission it reports — no `notifications/initialized` after it — is the
/// client's. Where neither rule decides (no `seq`, or an HTTP or lifecycle
/// event there) the finding is held against nobody rather than a guess.
fn attribute(findings: &mut [Finding], actor: Actor, context: &TraceContext<'_>) {
    for finding in findings {
        finding.party = Party::of_actor(actor).or_else(|| sender_at(context, finding.seq?));
    }
}

/// The party that sent the *message* at `seq`; `None` for any other event.
fn sender_at(context: &TraceContext<'_>, seq: u64) -> Option<Party> {
    let events = context.events();
    let index = events.binary_search_by_key(&seq, |event| event.seq).ok()?;
    let event = &events[index];
    event.message_payload()?;
    Some(Party::sender(event.direction))
}

/// One report per extension whose clauses the session exercised, in the order the
/// registry first names each extension.
///
/// An extension appears when one of its rows passed, failed, warned or is
/// unsupported. One whose every row is excluded, not applicable or not observed
/// has nothing to report about this session — it never negotiated or used the
/// extension, which is how an optional extension is usually met — and listing
/// its clauses anyway would print a block of nothing on every report.
fn extension_reports(registry: &Registry, context: &TraceContext<'_>) -> Vec<ExtensionReport> {
    let mut reports: Vec<ExtensionReport> = Vec::new();
    for requirement in registry.extension_requirements() {
        let extension = requirement.extension.as_deref().unwrap_or_default();
        let index = reports
            .iter()
            .position(|report| report.extension == extension)
            .unwrap_or_else(|| {
                reports.push(ExtensionReport {
                    extension: extension.to_owned(),
                    totals: Totals::default(),
                    requirements: Vec::new(),
                });
                reports.len() - 1
            });
        let row = build_row(requirement, registry.revision(), context);
        tally(&mut reports[index].totals, row.outcome);
        reports[index].requirements.push(row);
    }
    reports.retain(|report| {
        report.requirements.iter().any(|row| {
            matches!(
                row.outcome,
                Outcome::Pass | Outcome::Fail | Outcome::Warn | Outcome::Unsupported
            )
        })
    });
    reports
}

fn build_row(
    requirement: &Requirement,
    revision: ProtocolRevision,
    context: &TraceContext<'_>,
) -> RequirementReport {
    let mut row = RequirementReport {
        id: requirement.id.to_string(),
        level: requirement.level.keyword().to_owned(),
        actor: Some(requirement.actor),
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
                attribute(&mut row.findings, requirement.actor, context);
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
