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
//! and negotiates another ran under the negotiated one, and [`select`] judges it
//! there, saying so; but the client did state the proposal, so a caller who asks
//! for that revision instead (`--revision`) draws no note either.
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
use std::str::FromStr as _;

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::requirement::BUILTIN_REVISIONS;
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent, TransportKind};
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
    let probes = failed_discover_probes(events);
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
                if is_refused(payload, &refused) || probes.contains(&event.seq) {
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

/// The `seq` of every client `server/discover` request that no result answered.
///
/// A dual-era client "SHOULD probe with `server/discover`" and, on "`-32601`
/// Method not found, any other error, or [no response]", fall back to the
/// `initialize` handshake (`2026-07-28` stdio §Backward Compatibility). Such a
/// probe declares a revision the session then did not run under; against a
/// legacy Streamable HTTP server the refusal is often an HTTP 400 whose JSON-RPC
/// error carries `id: null`, which the id-keyed [`refused_request_ids`] cannot
/// attribute. A probe counts as answered only by a *result* carrying its id,
/// found scanning forward from the probe, so a later request reusing the id
/// cannot vouch for it.
pub(crate) fn failed_discover_probes(events: &[TraceEvent]) -> BTreeSet<u64> {
    let responses = Responses::of(events);
    let mut failed = BTreeSet::new();
    for (index, event) in events.iter().enumerate() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if payload.get("method").and_then(Value::as_str) != Some("server/discover") {
            continue;
        }
        if !responses.answered_by_result(events, index) {
            failed.insert(event.seq);
        }
    }
    failed
}

/// The `seq` of every modern request a dual-era client sent over Streamable
/// HTTP before falling back to `initialize`.
///
/// Such a client "MAY detect which era the server implements by attempting a
/// modern request first" and, refused, "fall back to `initialize` and continue
/// with the legacy version" (`2026-07-28` Streamable HTTP §Backward
/// Compatibility). An attempt is a request carrying the per-request `_meta`
/// revision, sent before the client's first `initialize`, that no result
/// answered. Whether the refusal was the `400` that procedure names, and
/// whether falling back was right, are that revision's rules to judge; what
/// this set records is that the request belongs to the modern era, not to the
/// handshake session that follows it.
pub(crate) fn failed_modern_attempts(events: &[TraceEvent]) -> BTreeSet<u64> {
    let responses = Responses::of(events);
    let mut failed = BTreeSet::new();
    for (index, event) in events.iter().enumerate() {
        if event.direction != Direction::ClientToServer
            || event.transport != TransportKind::StreamableHttp
        {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let Some(method) = payload.get("method").and_then(Value::as_str) else {
            continue;
        };
        if method == "initialize" {
            break;
        }
        let modern = payload
            .get("params")
            .and_then(|params| params.get("_meta"))
            .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
            .is_some();
        if modern && payload.get("id").is_some() && !responses.answered_by_result(events, index) {
            failed.insert(event.seq);
        }
    }
    failed
}

/// The requests a dual-era client sent to learn the server's era before
/// falling back to the handshake: failed stdio-style probes and failed modern
/// attempts over Streamable HTTP. They precede `initialize` by design.
pub(crate) fn era_detection(events: &[TraceEvent]) -> BTreeSet<u64> {
    let mut detection = failed_discover_probes(events);
    detection.extend(failed_modern_attempts(events));
    detection
}

/// Every response in a trace, indexed for "what answered this request first".
///
/// Built in one pass so each lookup is a binary search: scanning forward from
/// every candidate request instead made era detection quadratic, and a trace
/// of 40,000 unanswered pre-`initialize` requests took a minute to judge.
pub(crate) struct Responses {
    /// `(sent by the client, id in canonical form)` → `(index, carries
    /// result)` of each message with that id and no `method`, in trace order.
    /// Canonical form is coarser than value equality (it folds `1.0` into
    /// `1`), so a lookup confirms the id with `==`, the comparison this
    /// replaced.
    by_id: std::collections::BTreeMap<(bool, String), Vec<(usize, bool)>>,
}

impl Responses {
    pub(crate) fn of(events: &[TraceEvent]) -> Self {
        let mut by_id: std::collections::BTreeMap<(bool, String), Vec<(usize, bool)>> =
            std::collections::BTreeMap::new();
        for (index, event) in events.iter().enumerate() {
            let Some(payload) = event.message_payload() else {
                continue;
            };
            if payload.get("method").is_some() {
                continue;
            }
            let Some(id) = payload.get("id") else {
                continue;
            };
            by_id
                .entry((
                    event.direction == Direction::ClientToServer,
                    to_canonical_string(id),
                ))
                .or_default()
                .push((index, payload.get("result").is_some()));
        }
        Self { by_id }
    }

    /// Whether the request at `index` was answered by a *result* carrying its
    /// id: the first response from the other party with that id, after it, so
    /// a later request reusing the id cannot vouch for it.
    pub(crate) fn answered_by_result(&self, events: &[TraceEvent], index: usize) -> bool {
        self.result_for(events, index).is_some()
    }

    /// The `result` that answers the request at `index`, by the rule of
    /// [`Self::answered_by_result`]; `None` when nothing did, or an error did.
    pub(crate) fn result_for<'e>(
        &self,
        events: &'e [TraceEvent],
        index: usize,
    ) -> Option<&'e Value> {
        let request = &events[index];
        let id = request
            .message_payload()
            .and_then(|payload| payload.get("id"))?;
        let responder_is_client = request.direction != Direction::ClientToServer;
        let responses = self
            .by_id
            .get(&(responder_is_client, to_canonical_string(id)))?;
        let after = responses.partition_point(|&(at, _)| at <= index);
        let &(at, is_result) = responses[after..].iter().find(|&&(at, _)| {
            events[at]
                .message_payload()
                .and_then(|payload| payload.get("id"))
                == Some(id)
        })?;
        if !is_result {
            return None;
        }
        events[at].message_payload()?.get("result")
    }
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

mod selection;

pub use selection::{RevisionSource, Selection, UnjudgeableRevisions, select};

#[cfg(test)]
mod tests;
