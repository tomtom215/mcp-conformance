// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What protocol revision a session says it is, read from the session itself.
//!
//! A registry judges one revision. Point the validator at a recording of a
//! *different* one and every clause the two revisions disagree about becomes a
//! finding — confidently, with a verbatim spec quote, against an
//! implementation that violated nothing. A conforming `2026-07-28` stateless
//! session judged against `2025-11-25` fails `LIFE-001` for not opening with
//! `initialize`, which `2026-07-28` removes (SEP-2575), and `BASE-003` for
//! reusing request ids, which `2026-07-28` permits.
//!
//! The trace is not silent about this. Every revision states its own version on
//! the wire, and a recording carries it:
//!
//! - the `initialize` **result**'s `protocolVersion` — the negotiated revision,
//!   and the authority where there is one;
//! - the `initialize` **request**'s `protocolVersion` — what the client
//!   proposed, which is evidence even when no server answered;
//! - a request's `_meta` `io.modelcontextprotocol/protocolVersion` — how
//!   `2026-07-28` carries it, per request, having no handshake;
//! - the `MCP-Protocol-Version` HTTP header.
//!
//! So the validator can tell the difference between *this session broke the
//! rules* and *these are not the rules this session was playing by*, and
//! [`Report::revision_mismatch`] says which.
//!
//! **The rule is deliberately quiet**, in three ways.
//!
//! A mismatch is reported only when the session declared at least one revision
//! and *none* of them is the registry's. A session that proposes one revision
//! and negotiates another has touched both, so judging it against either is a
//! question worth asking and draws no note.
//!
//! A session that declares nothing at all — a message-level capture of a
//! handshake that never happened — gets no note either: there is nothing to
//! disagree with, and inventing a warning from an absence is the vacuous
//! reasoning this validator refuses everywhere else.
//!
//! And a version is only a declaration if the session actually ran under it.
//! Two filters enforce that. A request the other end answered with a JSON-RPC
//! **error** states nothing — it named a version and was told no — so a probe
//! asking for `1900-01-01` and drawing `-32022`, or a legacy `initialize`
//! drawing `-32601` from a server that no longer has one, are sessions of no
//! revision at all rather than of the one they asked for; the clauses that
//! judge those refusals (`TRAN-074`, `VERS-008`) are the ones with something to
//! say. And only revisions this build ships a registry for count, because the
//! note exists to send a reader to a registry that exists: *re-run with
//! `--revision X`* is worthless advice when there is no `X` to run against.
//!
//! [`Report::revision_mismatch`]: crate::report::Report::revision_mismatch

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr as _;

use mcp_conformance_core::requirement::BUILTIN_REVISIONS;
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::{EventBody, TraceEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How `2026-07-28` states the revision on each request, having no handshake.
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// The HTTP header carrying the revision on the Streamable HTTP transport.
const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

/// Every protocol revision the session states about itself, ascending.
///
/// Only well-formed `YYYY-MM-DD` values enter: a malformed `protocolVersion` is
/// a violation with its own clause (`LIFE-006`), not evidence of which revision
/// the session belongs to, and treating it as evidence would turn one finding
/// into two.
#[must_use]
pub fn declared_revisions(events: &[TraceEvent]) -> Vec<String> {
    collect(events)
        .into_iter()
        .map(|revision| revision.to_string())
        .collect()
}

/// [`declared_revisions`] before rendering, so comparisons stay typed: the
/// declarations this build ships a registry for.
fn collect(events: &[TraceEvent]) -> BTreeSet<ProtocolRevision> {
    collect_all(events)
        .into_iter()
        .filter(|revision| is_known(*revision))
        .collect()
}

/// Every well-formed revision the session states about itself, whether or not
/// any registry describes it.
fn collect_all(events: &[TraceEvent]) -> BTreeSet<ProtocolRevision> {
    let refused = refused_request_ids(events);
    let mut found: BTreeSet<ProtocolRevision> = BTreeSet::new();
    let mut pending_header: Option<&str> = None;
    for event in events {
        match &event.body {
            EventBody::Message { payload } => {
                // A request the other end answered with an error asserts
                // nothing about which rules the session ran under: it names a
                // version and is told no. Both corpus probes of that shape —
                // `1900-01-01` refused with `-32022`, and a legacy `initialize`
                // refused with `-32601` — would otherwise be read as sessions
                // of a revision that never happened. A response carries the
                // answer, so it is always evidence; so is a notification, which
                // has no id to be refused by.
                if is_refused(payload, &refused) {
                    pending_header = None;
                    continue;
                }
                collect_from_message(payload, &mut found);
                // The request's own headers travelled with it, so they stand or
                // fall together.
                if let Some(value) = pending_header.take() {
                    insert(value, &mut found);
                }
            }
            EventBody::Http { headers, .. } => {
                // Held until the message this request carried is seen: a
                // partial capture that recorded headers but no handshake still
                // states its revision this way, and a refused request must not.
                if let Some(value) = pending_header.take() {
                    insert(value, &mut found);
                }
                pending_header = headers.get(PROTOCOL_VERSION_HEADER).map(String::as_str);
            }
            _ => {}
        }
    }
    if let Some(value) = pending_header {
        insert(value, &mut found);
    }
    found
}

/// The ids of requests the other end answered with a JSON-RPC error.
fn refused_request_ids(events: &[TraceEvent]) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| event.message_payload())
        .filter(|payload| payload.get("error").is_some())
        .filter_map(|payload| payload.get("id"))
        .map(ToString::to_string)
        .collect()
}

/// Whether this message is a request whose id was answered with an error.
fn is_refused(payload: &Value, refused: &BTreeSet<String>) -> bool {
    payload.get("method").is_some()
        && payload
            .get("id")
            .is_some_and(|id| refused.contains(&id.to_string()))
}

/// The revisions a session declared, when it declared some and the registry's
/// is not among them. `None` means there is nothing to warn about.
#[must_use]
pub fn mismatch(registry_revision: ProtocolRevision, events: &[TraceEvent]) -> Option<Vec<String>> {
    let declared = collect(events);
    if declared.is_empty() || declared.contains(&registry_revision) {
        return None;
    }
    Some(
        declared
            .into_iter()
            .map(|revision| revision.to_string())
            .collect(),
    )
}

/// [`mismatch`] for a run judging several revisions at once: the note fires
/// only when none of them is one the session declared.
#[must_use]
pub fn mismatch_any(
    registry_revisions: &[ProtocolRevision],
    events: &[TraceEvent],
) -> Option<Vec<String>> {
    let declared = collect(events);
    if declared.is_empty()
        || registry_revisions
            .iter()
            .any(|revision| declared.contains(revision))
    {
        return None;
    }
    Some(
        declared
            .into_iter()
            .map(|revision| revision.to_string())
            .collect(),
    )
}

fn collect_from_message(payload: &Value, found: &mut BTreeSet<ProtocolRevision>) {
    // `initialize` states it in `params` (proposed) and in `result`
    // (negotiated); `2026-07-28` states it in every request's `params._meta`.
    // Reading both positions on every message needs no method dispatch and
    // cannot misattribute: no other member is spelled `protocolVersion` at the
    // top of an `initialize` envelope, and the `_meta` key is namespaced.
    for envelope in [payload.get("params"), payload.get("result")]
        .into_iter()
        .flatten()
    {
        if let Some(Value::String(version)) = envelope.get("protocolVersion") {
            insert(version, found);
        }
        if let Some(Value::String(version)) = envelope
            .get("_meta")
            .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
        {
            insert(version, found);
        }
    }
}

fn insert(value: &str, found: &mut BTreeSet<ProtocolRevision>) {
    if let Ok(revision) = ProtocolRevision::from_str(value) {
        found.insert(revision);
    }
}

/// Whether this build ships a registry for `revision`, and so could be asked to
/// judge against it.
///
/// A scan of [`BUILTIN_REVISIONS`], not a parse of the embedded registry: this
/// runs once per declaring message, and every `2026-07-28` request declares.
fn is_known(revision: ProtocolRevision) -> bool {
    BUILTIN_REVISIONS.contains(&revision)
}

/// How the revision a report judges against was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RevisionSource {
    /// The caller named it (`--revision`).
    Requested,
    /// The trace declared it: see [`declared_revisions`] for what counts.
    Declared,
    /// The trace declared nothing, so the newest supported revision was used.
    Default,
}

impl fmt::Display for RevisionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Requested => "requested",
            Self::Declared => "declared by the trace",
            Self::Default => "the trace declares none; newest supported",
        })
    }
}

/// The revisions to judge a trace against, and why those.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Selection {
    /// Ascending; never empty.
    pub revisions: Vec<ProtocolRevision>,
    /// How they were chosen.
    pub source: RevisionSource,
}

/// The trace declares only revisions no available registry describes.
///
/// Judging it anyway would report every clause the revisions disagree about as a
/// violation, against an implementation that may have violated nothing — so this
/// is an error the caller must resolve (a newer build, or an explicit revision),
/// never a silent fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnjudgeableRevisions {
    /// What the trace declared, ascending.
    pub declared: Vec<ProtocolRevision>,
    /// What the available registries describe, ascending.
    pub supported: Vec<ProtocolRevision>,
}

impl fmt::Display for UnjudgeableRevisions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let join = |revisions: &[ProtocolRevision]| {
            revisions
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        write!(
            f,
            "the trace declares protocol revision(s) {}, which no available registry \
             describes (supported: {}); judging it against another revision would \
             report that revision's rules as violations",
            join(&self.declared),
            join(&self.supported)
        )
    }
}

impl std::error::Error for UnjudgeableRevisions {}

/// Chooses the revisions to judge `events` against, from those `supported`.
///
/// 1. Declared revisions that are supported are judged — all of them, so a session
///    that proposed one revision and negotiated another is judged under both.
/// 2. A trace declaring only unsupported revisions is an error.
/// 3. A trace declaring nothing is judged against the newest supported revision.
///
/// # Errors
///
/// [`UnjudgeableRevisions`] in case 2, or when `supported` is empty.
pub fn select(
    supported: &[ProtocolRevision],
    events: &[TraceEvent],
) -> Result<Selection, UnjudgeableRevisions> {
    let declared = collect_all(events);
    let mut supported_sorted = supported.to_vec();
    supported_sorted.sort_unstable();
    supported_sorted.dedup();
    let judgeable: Vec<ProtocolRevision> = declared
        .iter()
        .copied()
        .filter(|revision| supported_sorted.contains(revision))
        .collect();
    if !judgeable.is_empty() {
        return Ok(Selection {
            revisions: judgeable,
            source: RevisionSource::Declared,
        });
    }
    match (declared.is_empty(), supported_sorted.last()) {
        (true, Some(&newest)) => Ok(Selection {
            revisions: vec![newest],
            source: RevisionSource::Default,
        }),
        _ => Err(UnjudgeableRevisions {
            declared: declared.into_iter().collect(),
            supported: supported_sorted,
        }),
    }
}

#[cfg(test)]
mod tests;
