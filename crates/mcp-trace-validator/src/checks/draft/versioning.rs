// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `2026-07-28` versioning and cross-era compatibility checks.
//!
//! The page states three rules a recorded session can bear on: what a client
//! does *after* being told which versions a server supports, the grammar of an
//! extension identifier, and what a modern-only server owes a legacy client it
//! has just refused. The remaining clauses bind extension-defined behaviour,
//! extension documentation, and client-side caching whose horizon outlives the
//! session; each carries a documented exclusion in the registry.
//!
//! Two of the page's clauses restate rules another page already states, and
//! name that page's check rather than a copy: VERS-001 shares
//! `transport.unsupported-version-error` (which was split from its HTTP-status
//! half precisely so this quote, which mentions no status, is not judged by
//! one) and VERS-003 shares `discover.implemented`.

use std::collections::BTreeSet;

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use super::super::base::validate_meta_key;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// `UnsupportedProtocolVersionError`.
const UNSUPPORTED_VERSION: i64 = -32022;

/// The `_meta` field a request declares its protocol version in.
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// The `_meta` field carrying the client's per-request capabilities.
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

/// The removed handshake a legacy client opens with.
const INITIALIZE: &str = "initialize";

/// The protocol version a request's `_meta` declares.
fn declared_version(payload: &Value) -> Option<&str> {
    payload
        .get("params")?
        .get("_meta")?
        .get(META_PROTOCOL_VERSION)?
        .as_str()
}

/// The versions an `UnsupportedProtocolVersionError` listed, when it listed any.
///
/// An empty or absent list is not treated as "no versions supported" — it is a
/// malformed error, which `transport.unsupported-version-error` reports against
/// the *server*. Judging the client on it would report a second party for the
/// first party's defect.
fn supported_from_error(payload: &Value) -> Option<BTreeSet<String>> {
    let error = payload.get("error")?;
    if error.get("code")?.as_i64()? != UNSUPPORTED_VERSION {
        return None;
    }
    let listed: BTreeSet<String> = error
        .get("data")?
        .get("supported")?
        .as_array()?
        .iter()
        .filter_map(|version| version.as_str().map(str::to_owned))
        .collect();
    (!listed.is_empty()).then_some(listed)
}

/// `VERS-002`: after being told which versions a server supports, a client's
/// requests use one of them.
///
/// The clause offers two branches — retry with a mutually supported version, or
/// surface an error — and only one of them puts anything on the wire. So this
/// judges the branch that does: a request sent *after* a `-32022` that listed
/// `data.supported`, declaring a version outside that list. The other branch
/// (the client stops) is indistinguishable from a session that simply ended,
/// and is not reported.
///
/// Every later request is judged, not only the immediate retry: "select a
/// mutually supported version" is not satisfied by a client that retries
/// correctly once and then reverts. A second `-32022` replaces the list, since
/// the newest statement is the server's current one.
pub(in crate::checks) fn retry_uses_supported_version(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let mut supported: Option<(u64, BTreeSet<String>)> = None;
    for (event, kind, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        match event.direction {
            Direction::ServerToClient => {
                if let Some(listed) = supported_from_error(payload) {
                    supported = Some((event.seq, listed));
                }
            }
            Direction::ClientToServer => {
                let (MessageKind::Request { .. }, Some((error_seq, listed))) =
                    (kind, supported.as_ref())
                else {
                    continue;
                };
                let Some(requested) = declared_version(payload) else {
                    continue;
                };
                // The subject is a request sent *after* the server stated its
                // versions: before that statement there is nothing to select
                // from, and a session that drew no such error is untested.
                sink.examined();
                if !listed.contains(requested) {
                    sink.push(
                        Some(event.seq),
                        format!(
                            "the request declares protocol version {requested:?}, which the \
                             `supported` list in the {UNSUPPORTED_VERSION} at seq {error_seq} \
                             does not offer ({})",
                            listed
                                .iter()
                                .map(|version| format!("{version:?}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                }
            }
        }
    }
}

/// Every extension identifier the trace advertises, as `(seq, surface, identifier)`.
///
/// The revision has exactly two capability surfaces, and this reads both: a
/// request's `_meta` client capabilities, and a `server/discover` result's
/// capabilities. The clause binds `both` actors, so judging only one side would
/// silently exempt the other.
fn extension_identifiers<'a>(
    context: &'a TraceContext<'_>,
) -> Vec<(u64, &'static str, &'a String)> {
    let mut out = Vec::new();
    let mut push = |seq, surface, capabilities: Option<&'a Value>| {
        let extensions = capabilities
            .and_then(|capabilities| capabilities.get("extensions"))
            .and_then(Value::as_object);
        if let Some(extensions) = extensions {
            out.extend(extensions.keys().map(|id| (seq, surface, id)));
        }
    };
    for (event, kind, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        match (event.direction, kind) {
            (
                Direction::ClientToServer,
                MessageKind::Request { .. } | MessageKind::Notification { .. },
            ) => push(
                event.seq,
                "client capabilities",
                payload
                    .get("params")
                    .and_then(|params| params.get("_meta"))
                    .and_then(|meta| meta.get(META_CLIENT_CAPABILITIES)),
            ),
            (Direction::ServerToClient, MessageKind::Result { .. }) => push(
                event.seq,
                "server capabilities",
                payload
                    .get("result")
                    .and_then(|result| result.get("capabilities")),
            ),
            _ => {}
        }
    }
    out
}

/// `VERS-004`: an extension identifier is a `_meta` key with a mandatory prefix.
///
/// The grammar half reuses [`validate_meta_key`] — the function, not the
/// `base.meta-key-format` check, which reads `_meta` keys in message envelopes
/// and would inspect no extension identifier at all. The clause's own addition
/// is the prefix: optional in a `_meta` key, required here, and the only way to
/// carry one is the `label(.label)*/` form the grammar already defines.
pub(in crate::checks) fn extension_identifier_format(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, surface, identifier) in extension_identifiers(context) {
        sink.examined();
        if let Err(reason) = validate_meta_key(identifier) {
            sink.push(
                Some(seq),
                format!("{surface} extension identifier {identifier:?} {reason}"),
            );
        } else if !identifier.contains('/') {
            sink.push(
                Some(seq),
                format!(
                    "{surface} extension identifier {identifier:?} has no prefix; the prefix \
                     is optional in a `_meta` key but mandatory for an extension identifier"
                ),
            );
        }
    }
}

/// Whether any string inside `value` contains a protocol-revision-shaped token.
///
/// Substring rather than whole-string, because the clause asks the server to
/// *name* its versions and says nothing about where: a `data.supported` array
/// and a `message` reading "this server speaks 2026-07-28" both name them.
fn names_a_revision(value: &Value) -> bool {
    match value {
        Value::String(text) => contains_revision(text),
        Value::Array(items) => items.iter().any(names_a_revision),
        Value::Object(members) => members.values().any(names_a_revision),
        _ => false,
    }
}

/// Whether `text` contains a `YYYY-MM-DD` token that is a real protocol revision.
fn contains_revision(text: &str) -> bool {
    (0..text.len()).any(|start| {
        text.get(start..start + 10)
            .is_some_and(|window| window.parse::<ProtocolRevision>().is_ok())
    })
}

/// `VERS-008`: a modern-only server names its versions when it refuses an
/// `initialize`.
///
/// The antecedent is witnessed by the refusal itself: a server that answers
/// `initialize` with an *error* is not serving the legacy era, and the trace is
/// being judged as a `2026-07-28` session — the same premise under which
/// DISC-001 reports a server that has no `server/discover`. A dual-era server
/// answers the handshake with a result and is never reached here.
///
/// "Name the protocol versions" is read as: some string in the error object
/// contains a real revision token. That admits both shapes a server might use —
/// `data.supported`, or a human-readable `message` — because the clause's
/// purpose is the diagnostic a legacy client can surface, not a wire format.
pub(in crate::checks) fn initialize_error_names_versions(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges_for(INITIALIZE) {
        if exchange.request.direction != Direction::ClientToServer {
            continue;
        }
        let Some(error) = exchange
            .response
            .message_payload()
            .and_then(|payload| payload.get("error"))
        else {
            continue;
        };
        // The subject is a *refused* `initialize`: the antecedent — a server
        // that does not serve the legacy era — is witnessed by the refusal.
        sink.examined();
        if !names_a_revision(error) {
            sink.push(
                Some(exchange.response.seq),
                "the error refusing `initialize` names no protocol version, leaving a legacy \
                 client — which has no fall-forward mechanism — nothing to surface"
                    .to_owned(),
            );
        }
    }
}
