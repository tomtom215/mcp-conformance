// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Wire-schema agreement: the official runner's `wire-schema-valid` check
//! reconciled with the validator's report `wire_schema` member, message by
//! message, over the sessions the runner drove (ADR-0020; review item 2.4).
//!
//! **What the runner reports.** From `0.2.0-alpha.11` on, every scenario that
//! sent or received a JSON-RPC message appends a `wire-schema-valid` check to
//! its `checks.json`: `SUCCESS` or `FAILURE`, with `details.messagesValidated`
//! (messages of both parties) and `details.violations` — one entry per invalid
//! message the *implementation under test* sent, carrying the message itself.
//! Invalid messages the runner itself sent go to a separate
//! `wire-schema-harness-error` check of the same shape (`suite.rs` reads
//! both). `0.1.16`, the stable pin, predates the check and reports neither;
//! its green run is then its whole verdict, and every message the validator
//! flags is a divergence from it, as a failing requirement row is.
//!
//! **How the two are joined.** The runner names no session, so a message is
//! identified by its content: the canonical JSON of the message (keys sorted,
//! integral floats written as integers — the runner's JavaScript serializer
//! writes `1.0` as `1`) and the party that sent it. A session's runner-side
//! outcome is derived from that join: `fail` when the runner flagged any of
//! its messages, else `pass` (`not-observed` when it carried none). Then:
//!
//! - a message the validator flags and the runner did not is `validator-only`;
//! - a message the runner flags that the validator passed where the tap
//!   recorded it, or that no tapped session carries at all, is `suite-only`.
//!
//! Every divergence must be explained in the committed ledger
//! (`schema_ledger.rs`), or the check fails; a ledger entry that explains
//! nothing in a compared run is stale and fails it too. A session-level
//! outcome disagreement always implies at least one message-level divergence
//! in that session, so the message level is what gates.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::trace::{Direction, TraceEvent};
use mcp_trace_validator::report::{Outcome, Report};
use serde::Serialize;
use serde_json::Value;

/// The runner's check for messages the implementation under test sent.
pub(crate) const SUITE_CHECK: &str = "wire-schema-valid";

/// The runner's check for messages it sent itself.
pub(crate) const SUITE_HARNESS_CHECK: &str = "wire-schema-harness-error";

/// Characters of a message kept in the artifact and in gate output.
const MESSAGE_PREVIEW: usize = 240;

/// One reconciliation leg: which run, and which party is under test.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Leg {
    /// The leg's name, as ledger entries spell it.
    pub(crate) name: &'static str,
    /// The direction the implementation under test sends in; the other
    /// direction is the runner (its "harness").
    pub(crate) implementation: Direction,
}

/// `cargo xtask conformance`'s server leg: the everything server under test.
pub(crate) const SERVER_LEG: Leg = Leg {
    name: "server",
    implementation: Direction::ServerToClient,
};

/// `cargo xtask conformance`'s client leg: the reference host under test.
pub(crate) const CLIENT_LEG: Leg = Leg {
    name: "client",
    implementation: Direction::ClientToServer,
};

/// `cargo xtask draft-readiness`'s legs, one per revision the server serves.
pub(crate) const DRAFT_LEGS: [(&str, Leg); 2] = [
    (
        "2025-11-25",
        Leg {
            name: "draft-2025-11-25",
            implementation: Direction::ServerToClient,
        },
    ),
    (
        "2026-07-28",
        Leg {
            name: "draft-2026-07-28",
            implementation: Direction::ServerToClient,
        },
    ),
];

/// Every leg name a ledger entry may name.
pub(crate) fn leg_names() -> Vec<&'static str> {
    let mut names = vec![SERVER_LEG.name, CLIENT_LEG.name];
    names.extend(DRAFT_LEGS.iter().map(|(_, leg)| leg.name));
    names
}

/// Which party sent a message, in the runner's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Origin {
    /// The implementation under test.
    Implementation,
    /// The runner itself.
    Harness,
}

impl Origin {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Implementation => "implementation",
            Self::Harness => "harness",
        }
    }

    fn of(leg: &Leg, direction: Direction) -> Self {
        if direction == leg.implementation {
            Self::Implementation
        } else {
            Self::Harness
        }
    }
}

/// One message the runner flagged.
#[derive(Debug, Clone)]
pub(crate) struct Flagged {
    pub(crate) scenario: String,
    pub(crate) origin: Origin,
    /// The message's canonical JSON ([`canonical`]).
    pub(crate) key: String,
    pub(crate) errors: Vec<String>,
}

/// The runner's side: what its wire-schema checks reported across the run.
#[derive(Debug, Default, Serialize)]
pub(crate) struct SuiteSide {
    /// Scenarios whose `checks.json` carries `wire-schema-valid`.
    pub(crate) scenarios: usize,
    /// Of those, the ones it reported `FAILURE`.
    pub(crate) failing: usize,
    /// `messagesValidated`, summed: messages of both parties.
    pub(crate) messages_validated: u64,
    /// Messages flagged, both checks together.
    pub(crate) flagged_messages: usize,
    #[serde(skip)]
    pub(crate) flagged: Vec<Flagged>,
}

/// One tapped session as the comparison sees it.
pub(crate) struct Session<'a> {
    pub(crate) trace: &'a str,
    pub(crate) events: &'a [TraceEvent],
    pub(crate) report: &'a Report,
}

/// How the two sides disagree about one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    /// The validator flags it; the runner did not.
    ValidatorOnly,
    /// The runner flags it; the validator passed it, or no tapped session
    /// carries it.
    SuiteOnly,
}

impl Kind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ValidatorOnly => "validator-only",
            Self::SuiteOnly => "suite-only",
        }
    }
}

/// One message-level divergence.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Divergence {
    pub(crate) kind: Kind,
    pub(crate) origin: Origin,
    /// The tapped session carrying the message; `None` when none does.
    pub(crate) trace: Option<String>,
    pub(crate) seq: Option<u64>,
    /// The runner scenarios that flagged it (suite-only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) scenarios: Vec<String>,
    /// The message, canonical and cut to a preview.
    pub(crate) message: String,
    /// Why the flagging side flagged it.
    pub(crate) detail: String,
    /// The full canonical message, which ledger filters match against.
    #[serde(skip)]
    pub(crate) key: String,
}

/// One session's outcome on each side.
#[derive(Debug, Serialize)]
pub(crate) struct SessionRow {
    pub(crate) trace: String,
    pub(crate) messages: u32,
    pub(crate) validator: &'static str,
    pub(crate) suite: &'static str,
    pub(crate) agree: bool,
}

/// The comparison before the ledger is consulted.
#[derive(Debug)]
pub(crate) struct Comparison {
    pub(crate) sessions: Vec<SessionRow>,
    pub(crate) divergences: Vec<Divergence>,
    /// Sessions the validator returned no `wire_schema` member for (no schema
    /// vendored for the judged revision) — never explainable, always gating.
    pub(crate) unjudged: Vec<String>,
}

/// A message as a join key: keys sorted, and every number with no fractional
/// part (within the integers an `f64` holds exactly) written as an integer.
pub(crate) fn canonical(value: &Value) -> String {
    normalize(value).to_string()
}

fn normalize(value: &Value) -> Value {
    match value {
        Value::Number(number) if number.is_f64() => {
            let float = number.as_f64().unwrap_or(f64::NAN);
            // 2^53: past it an f64 no longer holds every integer.
            if float.fract() == 0.0 && float.abs() < 9_007_199_254_740_992.0 {
                #[allow(clippy::cast_possible_truncation)] // integral and in range, above
                Value::from(float as i64)
            } else {
                value.clone()
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        Value::Object(map) => {
            let sorted: BTreeMap<&String, Value> = map
                .iter()
                .map(|(key, item)| (key, normalize(item)))
                .collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, item)| (key.clone(), item))
                    .collect(),
            )
        }
        _ => value.clone(),
    }
}

fn preview(key: &str) -> String {
    match key.char_indices().nth(MESSAGE_PREVIEW) {
        Some((cut, _)) => format!("{}…", &key[..cut]),
        None => key.to_owned(),
    }
}

/// A session's messages, as the validator counts them (objects and arrays).
fn messages(events: &[TraceEvent]) -> impl Iterator<Item = (&TraceEvent, &Value)> {
    events.iter().filter_map(|event| {
        event
            .message_payload()
            .filter(|payload| payload.is_object() || payload.is_array())
            .map(|payload| (event, payload))
    })
}

/// The runner's flagged messages by (sender, canonical message).
type SuiteIndex<'s> = BTreeMap<(Origin, String), Vec<&'s Flagged>>;

/// Compares the runner's flagged messages with the validator's, session by
/// session (module docs).
pub(crate) fn compare(leg: &Leg, suite: &SuiteSide, sessions: &[Session<'_>]) -> Comparison {
    let mut index: SuiteIndex<'_> = BTreeMap::new();
    for flagged in &suite.flagged {
        index
            .entry((flagged.origin, flagged.key.clone()))
            .or_default()
            .push(flagged);
    }
    let mut comparison = Comparison {
        sessions: Vec::new(),
        divergences: Vec::new(),
        unjudged: Vec::new(),
    };
    // The runner-flagged messages some tapped session carries.
    let mut found: BTreeSet<(Origin, String)> = BTreeSet::new();
    for session in sessions {
        if session.report.wire_schema.is_none() {
            comparison.unjudged.push(session.trace.to_owned());
            continue;
        }
        let row = compare_session(
            leg,
            &index,
            session,
            &mut found,
            &mut comparison.divergences,
        );
        comparison.sessions.push(row);
    }
    for ((origin, key), flagged) in &index {
        if !found.contains(&(*origin, key.clone())) {
            comparison.divergences.push(Divergence {
                kind: Kind::SuiteOnly,
                origin: *origin,
                trace: None,
                seq: None,
                scenarios: scenarios(flagged),
                message: preview(key),
                detail: format!(
                    "no tapped session carries this message; the runner said: {}",
                    suite_errors(flagged)
                ),
                key: key.clone(),
            });
        }
    }
    comparison
}

/// One session's messages against the index: its divergences are pushed onto
/// `divergences`, the runner-flagged messages it carries onto `found`.
fn compare_session(
    leg: &Leg,
    index: &SuiteIndex<'_>,
    session: &Session<'_>,
    found: &mut BTreeSet<(Origin, String)>,
    divergences: &mut Vec<Divergence>,
) -> SessionRow {
    let wire = session.report.wire_schema.as_ref();
    let ours: BTreeMap<u64, &str> = wire
        .into_iter()
        .flat_map(|wire| &wire.findings)
        .filter_map(|finding| finding.seq.map(|seq| (seq, finding.detail.as_str())))
        .collect();
    let mut suite_failed = false;
    let mut count = 0u32;
    for (event, payload) in messages(session.events) {
        count = count.saturating_add(1);
        let origin = Origin::of(leg, event.direction);
        let key = canonical(payload);
        let flagged = index.get(&(origin, key.clone()));
        if flagged.is_some() {
            suite_failed = true;
            found.insert((origin, key.clone()));
        }
        let (kind, detail, scenarios) = match (ours.get(&event.seq), flagged) {
            (Some(detail), None) => (Kind::ValidatorOnly, (*detail).to_owned(), Vec::new()),
            (None, Some(flagged)) => (Kind::SuiteOnly, suite_errors(flagged), scenarios(flagged)),
            _ => continue,
        };
        divergences.push(Divergence {
            kind,
            origin,
            trace: Some(session.trace.to_owned()),
            seq: Some(event.seq),
            scenarios,
            message: preview(&key),
            detail,
            key,
        });
    }
    let validator = match wire.map(|wire| wire.outcome) {
        Some(Outcome::Pass) => "pass",
        Some(Outcome::Fail) => "fail",
        Some(Outcome::NotObserved) => "not-observed",
        _ => "unjudged",
    };
    let suite = if count == 0 {
        "not-observed"
    } else if suite_failed {
        "fail"
    } else {
        "pass"
    };
    SessionRow {
        trace: session.trace.to_owned(),
        messages: count,
        validator,
        suite,
        agree: validator == suite,
    }
}

fn scenarios(flagged: &[&Flagged]) -> Vec<String> {
    let set: BTreeSet<&str> = flagged.iter().map(|f| f.scenario.as_str()).collect();
    set.into_iter().map(ToOwned::to_owned).collect()
}

fn suite_errors(flagged: &[&Flagged]) -> String {
    flagged
        .first()
        .map_or_else(String::new, |flagged| flagged.errors.join("; "))
}

mod suite;

#[cfg(test)]
use suite::read_checks;
pub(crate) use suite::read_suite;

#[cfg(test)]
mod tests;
