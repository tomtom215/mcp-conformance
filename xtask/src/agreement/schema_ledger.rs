// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The wire-schema divergence ledger and the gate it feeds: every
//! message-level divergence `schema.rs` finds is explained by a committed
//! entry or fails the run, and an entry that explains nothing in a compared
//! leg is stale and fails it too — the contract
//! `conformance/agreement-divergences.json` already holds for requirement
//! rows, applied to the schema.
//!
//! An entry names its leg, its kind (`validator-only` | `suite-only`), a
//! substring of the canonical message it explains (required: an entry that
//! matched every divergence of a kind would explain nothing in particular),
//! a triage class, an upstream link, and a reason. The classes are the
//! calibration policy's three plus `schema-pin-skew`: the runner bundles its
//! own copy of each revision's schema at a commit it does not publish, and
//! this validator vendors `wire_schema::UPSTREAM_COMMIT` with its own errata,
//! so a divergence can be the two copies differing rather than either side
//! misjudging the message. Such an entry says so.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::Path;

use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::report::Report;
use serde::{Deserialize, Serialize};

use super::schema::{self, Comparison, Divergence, Kind, Leg, Session, SessionRow, SuiteSide};

/// The committed ledger, relative to the workspace root.
pub(crate) const LEDGER: &str = "conformance/wire-schema-divergences.json";

/// The triage classes an entry may carry.
const CLASSES: [&str; 4] = ["our-bug", "suite-bug", "spec-ambiguity", "schema-pin-skew"];

/// One explained divergence. Unknown fields are rejected, so a misspelt
/// filter cannot silently widen an entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) leg: String,
    pub(crate) kind: String,
    /// `implementation` | `harness`; any when absent.
    #[serde(default)]
    pub(crate) origin: Option<String>,
    /// Substring of the canonical message.
    pub(crate) message_contains: String,
    /// Optional substring of the tapped session's file name.
    #[serde(default)]
    pub(crate) trace_contains: Option<String>,
    pub(crate) class: String,
    pub(crate) upstream: String,
    pub(crate) reason: String,
}

impl Entry {
    fn describe(&self) -> String {
        format!(
            "{} {} message_contains {:?}",
            self.leg, self.kind, self.message_contains
        )
    }

    fn explains(&self, leg: &Leg, divergence: &Divergence) -> bool {
        self.leg == leg.name
            && self.kind == divergence.kind.as_str()
            && self
                .origin
                .as_deref()
                .is_none_or(|origin| origin == divergence.origin.as_str())
            && divergence.key.contains(&self.message_contains)
            && self.trace_contains.as_deref().is_none_or(|needle| {
                divergence
                    .trace
                    .as_deref()
                    .is_some_and(|trace| trace.contains(needle))
            })
    }
}

/// The ledger file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ledger {
    /// The policy text, kept in the file so the contract travels with it.
    #[allow(dead_code)]
    pub(crate) policy: String,
    pub(crate) divergences: Vec<Entry>,
}

/// Parses and structurally validates a ledger.
pub(crate) fn parse(text: &str) -> Result<Ledger, String> {
    let ledger: Ledger = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let legs = schema::leg_names();
    for entry in &ledger.divergences {
        let name = entry.describe();
        if !legs.contains(&entry.leg.as_str()) {
            return Err(format!("entry {name}: leg must be one of {legs:?}"));
        }
        if ![Kind::ValidatorOnly, Kind::SuiteOnly]
            .iter()
            .any(|kind| kind.as_str() == entry.kind)
        {
            return Err(format!(
                "entry {name}: kind must be validator-only or suite-only"
            ));
        }
        if entry
            .origin
            .as_deref()
            .is_some_and(|origin| origin != "implementation" && origin != "harness")
        {
            return Err(format!(
                "entry {name}: origin must be implementation or harness"
            ));
        }
        if entry.message_contains.is_empty() {
            return Err(format!("entry {name}: message_contains must not be empty"));
        }
        if !CLASSES.contains(&entry.class.as_str()) {
            return Err(format!(
                "entry {name}: class {:?}; the ledger admits {CLASSES:?}",
                entry.class
            ));
        }
        if !entry.upstream.starts_with("https://") && !entry.upstream.starts_with("http://") {
            return Err(format!("entry {name}: upstream must be an http(s) URL"));
        }
        if entry.reason.trim().is_empty() {
            return Err(format!("entry {name}: reason must not be empty"));
        }
    }
    Ok(ledger)
}

fn load(path: &Path) -> Result<Ledger, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse(&text).map_err(|error| format!("{} is not valid: {error}", path.display()))
}

/// The leg's reconciliation, as written into the agreement artifact.
#[derive(Debug, Serialize)]
pub(crate) struct SchemaAgreement {
    pub(crate) leg: &'static str,
    /// Whether the runner reported `wire-schema-valid` at all this run; when
    /// it did not, nothing was compared and `note` says why.
    pub(crate) compared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
    pub(crate) suite: SuiteSide,
    pub(crate) sessions: Vec<SessionRow>,
    pub(crate) explained: Vec<Divergence>,
    pub(crate) unexplained: Vec<Divergence>,
    pub(crate) unjudged_sessions: Vec<String>,
    pub(crate) stale_ledger_entries: Vec<String>,
}

impl SchemaAgreement {
    /// The gate message, when the reconciliation demands one.
    pub(crate) fn gate_error(&self) -> Option<String> {
        let mut sections = Vec::new();
        if !self.unexplained.is_empty() {
            let listing = self
                .unexplained
                .iter()
                .map(|divergence| {
                    format!(
                        "  {} ({}) on {}{}: {} — {}",
                        divergence.kind.as_str(),
                        divergence.origin.as_str(),
                        divergence.trace.as_deref().unwrap_or("no tapped session"),
                        divergence
                            .seq
                            .map_or_else(String::new, |seq| format!(" seq {seq}")),
                        divergence.message,
                        divergence.detail
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!(
                "{} unexplained wire-schema divergence(s) on the {} leg:\n{listing}\n\
                 Triage each (our-bug | suite-bug | spec-ambiguity | schema-pin-skew), \
                 fix or file upstream, and record explained ones in {LEDGER}",
                self.unexplained.len(),
                self.leg
            ));
        }
        if !self.unjudged_sessions.is_empty() {
            sections.push(format!(
                "{} session(s) on the {} leg have no wire_schema member (no schema \
                 vendored for the judged revision): {}",
                self.unjudged_sessions.len(),
                self.leg,
                self.unjudged_sessions.join(", ")
            ));
        }
        if !self.stale_ledger_entries.is_empty() {
            sections.push(format!(
                "{} stale entr(y/ies) in {LEDGER} — each explains a wire-schema \
                 divergence that no longer occurs:\n  {}\nRemove them in this same change",
                self.stale_ledger_entries.len(),
                self.stale_ledger_entries.join("\n  ")
            ));
        }
        (!sections.is_empty()).then(|| sections.join("\n"))
    }

    /// The one-line summary the task prints.
    pub(crate) fn summary(&self) -> String {
        let agreeing = self.sessions.iter().filter(|row| row.agree).count();
        format!(
            "wire-schema agreement ({}) — runner: {} scenario(s), {} message(s) \
             validated, {} flagged; validator: {} session(s), {} message(s), {} \
             flagged; sessions agreeing {agreeing}/{}; divergences {} explained, \
             {} unexplained; {} stale ledger entr(y/ies){}",
            self.leg,
            self.suite.scenarios,
            self.suite.messages_validated,
            self.suite.flagged_messages,
            self.sessions.len(),
            self.sessions
                .iter()
                .map(|row| u64::from(row.messages))
                .sum::<u64>(),
            self.sessions
                .iter()
                .filter(|row| row.validator == "fail")
                .count(),
            self.sessions.len(),
            self.explained.len(),
            self.unexplained.len(),
            self.stale_ledger_entries.len(),
            self.note
                .as_deref()
                .map_or_else(String::new, |note| format!(" — {note}")),
        )
    }
}

/// Reconciles a comparison against the ledger's entries for `leg`. The
/// runner side is `compared` when it reported `wire-schema-valid` at all;
/// when it did not, a `suite-only` entry cannot have matched and is not
/// called stale for it.
pub(crate) fn reconcile(
    leg: &Leg,
    ledger: &Ledger,
    suite: SuiteSide,
    comparison: Comparison,
) -> SchemaAgreement {
    let compared = suite.scenarios > 0;
    let entries: Vec<&Entry> = ledger
        .divergences
        .iter()
        .filter(|entry| entry.leg == leg.name)
        .collect();
    let (explained, unexplained): (Vec<_>, Vec<_>) = comparison
        .divergences
        .into_iter()
        .partition(|divergence| entries.iter().any(|entry| entry.explains(leg, divergence)));
    let stale = entries
        .iter()
        .filter(|entry| compared || entry.kind != Kind::SuiteOnly.as_str())
        .filter(|entry| {
            !explained
                .iter()
                .any(|divergence| entry.explains(leg, divergence))
        })
        .map(|entry| entry.describe())
        .collect();
    SchemaAgreement {
        leg: leg.name,
        compared,
        note: (!compared).then(|| {
            format!(
                "the runner reported no `{}` check in any scenario (it first shipped \
                 in 0.2.0-alpha.11), so every validator finding stands against its \
                 green verdict as a validator-only divergence",
                schema::SUITE_CHECK
            )
        }),
        suite,
        sessions: comparison.sessions,
        explained,
        unexplained,
        unjudged_sessions: comparison.unjudged,
        stale_ledger_entries: stale,
    }
}

/// Runs the whole leg: reads the runner's results, compares, reconciles
/// against the committed ledger. When the runner reported no
/// `wire-schema-valid` (the `0.1.16` pin predates it), its green run is the
/// whole of its verdict: a message the validator flags is still a
/// `validator-only` divergence, exactly as a failing requirement row on a
/// green run is in the row-level agreement.
pub(crate) fn check(
    workspace_root: &Path,
    leg: &Leg,
    results_dir: &Path,
    sessions: &[Session<'_>],
) -> Result<SchemaAgreement, String> {
    let ledger = load(&workspace_root.join(LEDGER))?;
    let suite = schema::read_suite(results_dir)?;
    let comparison = schema::compare(leg, &suite, sessions);
    Ok(reconcile(leg, &ledger, suite, comparison))
}

/// The wire-schema reconciliation alone, for a run whose sessions no
/// requirement-row agreement covers: `cargo xtask draft-readiness`'s legs,
/// which drive the runner's `2026-07-28` scenarios and so are judged against
/// that revision's registry and schema whatever the server serves. The
/// reconciliation is written to `<results_dir>/wire-schema-agreement.json`.
///
/// # Errors
///
/// As [`run`]: unreadable artifacts, a malformed ledger, an unexplained
/// divergence, or a stale ledger entry.
pub(crate) fn run_wire_schema_only(
    workspace_root: &Path,
    tap_dir: &Path,
    results_dir: &Path,
    suite_version: &str,
    revision: &str,
    leg: &schema::Leg,
) -> Result<(), String> {
    let set = mcp_conformance_core::requirement::RegistrySet::builtin()
        .map_err(|error| format!("embedded registry set failed to load: {error}"))?;
    let registry = revision
        .parse()
        .ok()
        .and_then(|revision| set.registry(revision))
        .ok_or_else(|| format!("the registry set does not describe {revision}"))?;
    let sessions = super::artifacts::load_sessions(tap_dir)?;
    if sessions.is_empty() {
        return Err(format!("no tapped sessions found in {}", tap_dir.display()));
    }
    let (_, _, reports) = super::validate_sessions(&registry, &sessions);
    let wire_schema = check(
        workspace_root,
        leg,
        results_dir,
        &session_views(&sessions, &reports),
    )?;
    let path = results_dir.join("wire-schema-agreement.json");
    let json = serde_json::to_string_pretty(&wire_schema)
        .map_err(|error| format!("wire-schema artifact unserializable: {error}"))?;
    std::fs::write(&path, json + "\n")
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    eprintln!(
        "xtask: agreement — suite {suite_version}: {}",
        wire_schema.summary()
    );
    wire_schema.gate_error().map_or(Ok(()), Err)
}

/// Pairs each session with its report for the wire-schema comparison.
pub(super) fn session_views<'a>(
    sessions: &'a [(String, Vec<TraceEvent>)],
    reports: &'a [(String, Report)],
) -> Vec<schema::Session<'a>> {
    sessions
        .iter()
        .zip(reports)
        .map(|((trace, events), (_, report))| schema::Session {
            trace,
            events,
            report,
        })
        .collect()
}
