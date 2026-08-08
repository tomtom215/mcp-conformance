// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `2026-07-28` rejection clauses a *server* owns: what it must refuse, with
//! which JSON-RPC code, and under which HTTP status.
//!
//! These judge answers rather than requests. A malformed POST is the client's
//! defect and is reported by [`super::headers`]; what is reported here is the
//! server having seen that POST and answered it anyway — which the recorded
//! exchange shows directly.

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::super::FindingSink;
use super::super::http_status_for;
use super::{
    Match, Post, compare, designations_by_tool, header_safe, mirrors, posts, posts_by_message,
    sentinel_payload,
};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// `HeaderMismatch`.
const HEADER_MISMATCH: i64 = -32020;
/// `UnsupportedProtocolVersionError`.
const UNSUPPORTED_VERSION: i64 = -32022;
/// JSON-RPC `Method not found`.
const METHOD_NOT_FOUND: i64 = -32601;

/// The JSON-RPC error code an exchange's answer carried, if it was an error.
fn answer_code(response: &mcp_conformance_core::trace::TraceEvent) -> Option<i64> {
    response
        .message_payload()?
        .get("error")?
        .get("code")?
        .as_i64()
}

/// How a finding names the answer a request drew.
fn answer_label(code: Option<i64>) -> String {
    code.map_or_else(|| "a result".to_owned(), |code| format!("error {code}"))
}

/// `TRAN-073`: a protocol-version header/body mismatch is rejected.
pub(in crate::checks) fn version_mismatch_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    rejected_for(context, sink, version_mismatch_fault);
}

/// `TRAN-096`: a recognized `Mcp-Param-*` carrying invalid characters is rejected.
pub(in crate::checks) fn invalid_param_header_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let recognized = recognized_param_headers(context);
    rejected_for(context, sink, |post| invalid_param_fault(post, &recognized));
}

/// `TRAN-098`/`TRAN-102`: whatever draws `HeaderMismatch` draws HTTP `400`.
///
/// The two clauses state one rule in two sections — the response shape a header
/// rejection takes — so they share a check; which *antecedent* obliged the
/// rejection is TRAN-073's and TRAN-096's to report.
pub(in crate::checks) fn header_mismatch_status(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, _, _) in context.messages() {
        if answer_code(event) != Some(HEADER_MISMATCH) {
            continue;
        }
        if let Some((status_seq, status)) = http_status_for(context, event.seq)
            && status != 400
        {
            sink.push(
                Some(status_seq),
                format!(
                    "HeaderMismatch ({HEADER_MISMATCH}) was returned with HTTP {status}, not 400"
                ),
            );
        }
    }
}

/// Reports every exchange whose POST carried `fault` yet drew something other
/// than a `HeaderMismatch` rejection — the shared body of TRAN-073 and TRAN-096.
fn rejected_for(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    fault: impl Fn(&Post<'_>) -> Option<String>,
) {
    let by_message = posts_by_message(context);
    for exchange in context.exchanges() {
        let Some(post) = by_message.get(&exchange.request.seq) else {
            continue;
        };
        let Some(reason) = fault(post) else {
            continue;
        };
        let code = answer_code(exchange.response);
        if code != Some(HEADER_MISMATCH) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the POST at seq {} {reason}; the server answered with {} instead of \
                     rejecting it with {HEADER_MISMATCH} (HeaderMismatch)",
                    post.seq,
                    answer_label(code)
                ),
            );
        }
    }
}

/// The `Mcp-Param-*` headers the trace shows a tool declaring — the ones a
/// server demonstrably *recognizes*, which is what TRAN-096 is scoped to.
fn recognized_param_headers(context: &TraceContext<'_>) -> BTreeSet<String> {
    designations_by_tool(context)
        .values()
        .flatten()
        .map(|designation| designation.header.clone())
        .collect()
}

/// TRAN-073's antecedent: the protocol-version header disagrees with `_meta`.
fn version_mismatch_fault(post: &Post<'_>) -> Option<String> {
    let sent = post.headers.get("mcp-protocol-version")?;
    let body = post.body_protocol_version()?;
    (sent != body).then(|| {
        format!("carried `MCP-Protocol-Version: {sent}` against a body `_meta` version of {body:?}")
    })
}

/// TRAN-096's antecedent: a recognized custom header carries characters no
/// header value may hold unencoded.
fn invalid_param_fault(post: &Post<'_>, recognized: &BTreeSet<String>) -> Option<String> {
    post.headers
        .iter()
        .find(|(name, value)| {
            recognized.contains(*name) && sentinel_payload(value).is_none() && !header_safe(value)
        })
        .map(|(name, value)| {
            format!("carried `{name}: {value:?}`, whose characters are not valid unencoded")
        })
}

/// `TRAN-097`/`TRAN-100`: a header/body mismatch is validated and rejected.
///
/// The comparison the server is required to perform, performed on the trace and
/// then held against the answer it gave: a POST whose mirrored header disagrees
/// with the body field it mirrors — after decoding the sentinel, as TRAN-091 and
/// TRAN-103 require — must not have drawn a result.
pub(in crate::checks) fn header_body_match_validated(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let designated = designations_by_tool(context);
    let by_message = posts_by_message(context);
    for exchange in context.exchanges() {
        let Some(post) = by_message.get(&exchange.request.seq) else {
            continue;
        };
        if exchange.result.is_none() {
            continue; // rejected; whether it was rejected *correctly* is TRAN-098's
        }
        for mirror in mirrors(post, &designated) {
            let Some(sent) = post.headers.get(&mirror.header) else {
                continue;
            };
            if compare(sent, &mirror.value) == Match::Mismatch {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "the POST at seq {} carried `{}: {sent}` against `{}` = {:?}; the \
                         server answered it with a result instead of rejecting the mismatch",
                        post.seq, mirror.label, mirror.source, mirror.value
                    ),
                );
            }
        }
    }
}

/// `TRAN-074`: an unsupported protocol version draws
/// `UnsupportedProtocolVersionError`, listing the versions the server supports.
pub(in crate::checks) fn unsupported_version_error(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    unsupported_version_shape(context, sink);
    unsupported_version_answer(context, sink);
}

/// The answer side: `-32022` lists the supported versions and carries HTTP 400.
fn unsupported_version_shape(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        if answer_code(event) != Some(UNSUPPORTED_VERSION) {
            continue;
        }
        let lists_versions = event
            .message_payload()
            .and_then(|payload| payload.get("error"))
            .and_then(|error| error.get("data"))
            .and_then(|data| data.get("supported"))
            .and_then(Value::as_array)
            .is_some_and(|supported| {
                !supported.is_empty() && supported.iter().all(Value::is_string)
            });
        if !lists_versions {
            sink.push(
                Some(event.seq),
                format!(
                    "error {UNSUPPORTED_VERSION} does not carry `data.supported` listing the \
                     protocol versions the server does implement"
                ),
            );
        }
        if let Some((status_seq, status)) = http_status_for(context, event.seq)
            && status != 400
        {
            sink.push(
                Some(status_seq),
                format!(
                    "UnsupportedProtocolVersionError ({UNSUPPORTED_VERSION}) was returned \
                     with HTTP {status}, not 400"
                ),
            );
        }
    }
}

/// The obligation side: with the server's own supported list on the wire, a
/// request carrying a version outside it must draw `-32022`.
///
/// The list comes from a `server/discover` result the trace itself carried, so
/// this judges the server against what it said about itself — never against an
/// assumption about which versions it ought to implement. Applied to the whole
/// trace, not just what follows discovery: which versions a server implements is
/// a property of the server, not of when the client asked.
fn unsupported_version_answer(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some(supported) = declared_versions(context) else {
        return;
    };
    let by_message = posts_by_message(context);
    for exchange in context.exchanges() {
        let Some(post) = by_message.get(&exchange.request.seq) else {
            continue;
        };
        let Some(requested) = post.body_protocol_version() else {
            continue;
        };
        if supported.contains(requested) {
            continue;
        }
        let code = answer_code(exchange.response);
        if code != Some(UNSUPPORTED_VERSION) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the POST at seq {} requested protocol version {requested:?}, which the \
                     server's own `supportedVersions` omits; it answered with {} instead of \
                     {UNSUPPORTED_VERSION}",
                    post.seq,
                    answer_label(code)
                ),
            );
        }
    }
}

/// The protocol versions a `server/discover` result in the trace declared.
fn declared_versions(context: &TraceContext<'_>) -> Option<BTreeSet<String>> {
    context
        .exchanges_for("server/discover")
        .find_map(|exchange| {
            let versions: BTreeSet<String> = exchange
                .result?
                .get("supportedVersions")?
                .as_array()?
                .iter()
                .filter_map(|version| version.as_str().map(str::to_owned))
                .collect();
            (!versions.is_empty()).then_some(versions)
        })
}

/// `TRAN-075`: an unimplemented method draws `404 Not Found` with `-32601`.
pub(in crate::checks) fn unknown_method_404(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // A POST is the only way to reach the endpoint at this revision, so a trace
    // without HTTP framing carries no status to judge and reports nothing.
    if posts(context).is_empty() {
        return;
    }
    for (event, _, _) in context.messages() {
        if answer_code(event) != Some(METHOD_NOT_FOUND) {
            continue;
        }
        if let Some((status_seq, status)) = http_status_for(context, event.seq)
            && status != 404
        {
            sink.push(
                Some(status_seq),
                format!(
                    "`Method not found` ({METHOD_NOT_FOUND}) was returned with HTTP {status}, \
                     not 404"
                ),
            );
        }
    }
}
