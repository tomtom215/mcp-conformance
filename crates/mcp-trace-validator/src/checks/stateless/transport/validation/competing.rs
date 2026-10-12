// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Requests that more than one defined rejection fits.
//!
//! The revision defines a rejection for each fault a request can carry, and
//! states no order among them. A request can carry several at once — an
//! `MCP-Protocol-Version` header disagreeing with a body `_meta` that also names
//! a version the server does not implement, say — and then several MUSTs bind
//! the one answer: TRAN-073 ("the server MUST reject the request with `400 Bad
//! Request` and a `HeaderMismatch` JSON-RPC error"), TRAN-074 and VERS-001 ("it
//! MUST respond with \[…\] an `UnsupportedProtocolVersionError`"), and
//! `basic/index`'s envelope rule ("A request missing any required field is
//! malformed; the server MUST reject it with JSON-RPC error code `-32602`").
//! An answer carries one code, so no server can satisfy all of them; holding it
//! to whichever one a check was written for convicted every possible answer.
//!
//! So each clause accepts the code of any clause the same request falls under.
//! A result, or a code none of them defines, is still a failure to reject.
//!
//! The version lists those faults are judged against live here too: a version
//! is unsupported only by the server's own account, never by an assumption
//! about what it ought to implement.

use std::collections::BTreeSet;

use mcp_conformance_core::trace::TraceEvent;
use serde_json::Value;

use super::super::{META_PROTOCOL_VERSION, Post};
use super::{
    DISCOVER, HEADER_MISMATCH, INVALID_PARAMS, UNSUPPORTED_VERSION, invalid_param_fault,
    version_mismatch_fault,
};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The `_meta` fields `basic/index` requires on every client request.
const REQUIRED_META: [&str; 2] = [
    META_PROTOCOL_VERSION,
    "io.modelcontextprotocol/clientCapabilities",
];

/// Which of the server's defined rejections one request has earned.
#[derive(Debug, Clone, Copy)]
pub(super) struct Faults {
    /// A header disagrees with the body or holds invalid characters — TRAN-073's
    /// and TRAN-096's antecedents, both answered with `HeaderMismatch`.
    header: bool,
    /// The request names a version the server says it does not implement, in
    /// its body `_meta` *or* its header: when the two differ, which of them is
    /// "the requested protocol version" the text does not say.
    unsupported: bool,
    /// The body `_meta` lacks a field `basic/index` requires.
    malformed: bool,
}

impl Faults {
    /// The faults of the request whose `params` are given, sent in `post` when
    /// the trace pairs one, judged against the versions the server `supported`
    /// and the `Mcp-Param-*` headers it `recognized`.
    pub(super) fn of(
        params: Option<&Value>,
        post: Option<&Post<'_>>,
        supported: Option<&BTreeSet<String>>,
        recognized: &BTreeSet<String>,
    ) -> Self {
        let meta = params.and_then(|params| params.get("_meta"));
        let header = post.is_some_and(|post| {
            version_mismatch_fault(post).is_some()
                || invalid_param_fault(post, recognized).is_some()
        });
        let body_version = meta
            .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
            .and_then(Value::as_str);
        let header_version = post
            .and_then(|post| post.headers.get("mcp-protocol-version"))
            .map(String::as_str);
        let unsupported = supported.is_some_and(|supported| {
            [body_version, header_version]
                .into_iter()
                .flatten()
                .any(|version| !supported.contains(version))
        });
        let malformed = REQUIRED_META
            .iter()
            .any(|field| meta.and_then(|meta| meta.get(field)).is_none());
        Self {
            header,
            unsupported,
            malformed,
        }
    }

    /// Whether an answer carrying `code` is a rejection these faults define.
    pub(super) const fn admit(self, code: Option<i64>) -> bool {
        match code {
            Some(HEADER_MISMATCH) => self.header,
            Some(UNSUPPORTED_VERSION) => self.unsupported,
            Some(INVALID_PARAMS) => self.malformed,
            _ => false,
        }
    }
}

/// The protocol versions a `server/discover` result in the trace declared.
pub(super) fn declared_versions(context: &TraceContext<'_>) -> Option<BTreeSet<String>> {
    context
        .exchanges_for(DISCOVER)
        .find_map(|exchange| versions(exchange.result?.get("supportedVersions")?))
}

/// The protocol versions an `UnsupportedProtocolVersionError` listed.
pub(super) fn listed_versions(response: &TraceEvent) -> Option<BTreeSet<String>> {
    versions(
        response
            .message_payload()?
            .get("error")?
            .get("data")?
            .get("supported")?,
    )
}

/// A list of version strings; `None` when it names none, which says nothing.
fn versions(list: &Value) -> Option<BTreeSet<String>> {
    let versions: BTreeSet<String> = list
        .as_array()?
        .iter()
        .filter_map(|version| version.as_str().map(str::to_owned))
        .collect();
    (!versions.is_empty()).then_some(versions)
}
