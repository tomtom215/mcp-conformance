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
use std::str::FromStr as _;

use mcp_conformance_core::requirement::RegistrySet;
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::{EventBody, TraceEvent};
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

/// [`declared_revisions`] before rendering, so comparisons stay typed.
fn collect(events: &[TraceEvent]) -> BTreeSet<ProtocolRevision> {
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
    if let Ok(revision) = ProtocolRevision::from_str(value)
        && is_known(revision)
    {
        found.insert(revision);
    }
}

/// Whether this build ships a registry for `revision`, and so could be asked to
/// judge against it. Feature-dependent by construction: a build without
/// `draft-2026-07-28` cannot judge that revision and therefore has no advice to
/// offer about a recording of it.
fn is_known(revision: ProtocolRevision) -> bool {
    RegistrySet::builtin().is_ok_and(|set| set.revisions().contains(&revision))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::reader::{Limits, parse_trace};

    fn events(document: &str) -> Vec<TraceEvent> {
        parse_trace(document, &Limits::default()).unwrap()
    }

    fn rev(revision: &str) -> ProtocolRevision {
        revision.parse().unwrap()
    }

    const HANDSHAKE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;

    #[test]
    fn the_handshake_states_the_revision_from_both_ends() {
        assert_eq!(declared_revisions(&events(HANDSHAKE)), ["2025-11-25"]);
        assert!(mismatch(rev("2025-11-25"), &events(HANDSHAKE)).is_none());
    }

    // Needs a second shipped registry: without `draft-2026-07-28` this build
    // has none, so `2026-07-28` is not a revision it could be asked to judge
    // and correctly counts as no declaration at all.
    #[test]
    #[cfg(feature = "draft-2026-07-28")]
    fn a_stateless_session_states_it_per_request() {
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}}"#;
        assert_eq!(declared_revisions(&events(document)), ["2026-07-28"]);
        assert_eq!(
            mismatch(rev("2025-11-25"), &events(document)),
            Some(vec!["2026-07-28".to_owned()])
        );
    }

    #[test]
    fn the_http_header_states_it_too() {
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"2025-11-25"}}"#;
        assert_eq!(declared_revisions(&events(document)), ["2025-11-25"]);
    }

    // Needs a second shipped registry: without `draft-2026-07-28` this build
    // has none, so `2026-07-28` is not a revision it could be asked to judge
    // and correctly counts as no declaration at all.
    #[test]
    #[cfg(feature = "draft-2026-07-28")]
    fn a_session_that_touched_the_registrys_revision_draws_no_note() {
        // Proposed one revision, negotiated another: judging it against either
        // is a fair question, so neither draws a warning.
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;
        assert_eq!(
            declared_revisions(&events(document)),
            ["2025-11-25", "2026-07-28"]
        );
        assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
        assert!(mismatch(rev("2026-07-28"), &events(document)).is_none());
    }

    #[test]
    fn a_session_that_declares_nothing_draws_no_note() {
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;
        assert!(declared_revisions(&events(document)).is_empty());
        assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
    }

    #[test]
    fn a_malformed_version_is_not_evidence_of_a_revision() {
        // LIFE-006's subject, not this module's: a value that is not a dated
        // revision says nothing about which revision the session belongs to.
        let document = r#"{"seq":0,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"draft","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;
        assert!(declared_revisions(&events(document)).is_empty());
        assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
    }

    #[test]
    fn a_version_this_build_cannot_judge_is_not_a_declaration() {
        // Well-formed, but no registry ships for it, so `--revision 1900-01-01`
        // would be advice with nothing behind it.
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"1900-01-01"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}}"#;
        assert!(declared_revisions(&events(document)).is_empty());
        assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
    }

    // Needs a second shipped registry: without `draft-2026-07-28` this build
    // has none, so `2026-07-28` is not a revision it could be asked to judge
    // and correctly counts as no declaration at all.
    #[test]
    #[cfg(feature = "draft-2026-07-28")]
    fn a_refused_request_states_no_revision() {
        // The `vers-008` corpus trace: a legacy client's `initialize` reaches a
        // server that no longer implements one. The session ran under no
        // revision, and `VERS-008` is the clause with something to say about
        // it — this module must not add "you used the wrong registry" on top.
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}}"#;
        assert!(declared_revisions(&events(document)).is_empty());
        assert!(mismatch(rev("2026-07-28"), &events(document)).is_none());
    }

    #[test]
    fn a_headers_only_capture_still_states_its_revision() {
        // A recording that began mid-session has no handshake to read, but
        // every request still carries the header.
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"2025-11-25"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"method":"tools/list"}}
{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"result":{"tools":[]}}}"#;
        assert_eq!(declared_revisions(&events(document)), ["2025-11-25"]);
    }

    #[test]
    fn a_refused_request_takes_its_own_header_down_with_it() {
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"2025-11-25"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"method":"tools/list"}}
{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"error":{"code":-32022,"message":"Unsupported protocol version"}}}"#;
        assert!(declared_revisions(&events(document)).is_empty());
    }

    #[test]
    fn a_non_string_version_is_ignored_rather_than_stringified() {
        let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":20251125}}}"#;
        assert!(declared_revisions(&events(document)).is_empty());
    }

    // Needs a second shipped registry: without `draft-2026-07-28` this build
    // has none, so `2026-07-28` is not a revision it could be asked to judge
    // and correctly counts as no declaration at all.
    #[test]
    #[cfg(feature = "draft-2026-07-28")]
    fn declarations_are_deduplicated_and_ordered() {
        let document = format!(
            "{HANDSHAKE}\n{}",
            r#"{"seq":2,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}}"#
        );
        assert_eq!(
            declared_revisions(&events(&document)),
            ["2025-11-25", "2026-07-28"]
        );
    }
}
