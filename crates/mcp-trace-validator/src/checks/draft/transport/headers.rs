// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `2026-07-28` request-header clauses a *client* owns: which headers a POST
//! carries, that their values are encoded safely, and that a tool's
//! `x-mcp-header` annotations are usable at all.
//!
//! Every POST clause here is scoped to POSTs carrying a JSON-RPC *request*.
//! That is the revision's own boundary — "header requirements for notification
//! POSTs are not defined by this revision" (`#sending-messages`) — not a
//! convenience. [`x_mcp_header_name_valid`] is the one exception, and only
//! because it judges a tool definition rather than a POST.

use std::collections::BTreeSet;

use super::super::super::FindingSink;
use super::{
    Designation, Match, Post, compare, designations, designations_by_tool, header_safe,
    is_miscased_sentinel, mirrors, posts, sentinel_payload, tool_definitions,
};
use crate::context::TraceContext;

/// RFC 9110 §5.1 `tchar`, the punctuation half.
const TCHAR: &[u8] = b"!#$%&'*+-.^_`|~";

/// The JSON Schema types an `x-mcp-header` annotation may sit on.
const PRIMITIVE_TYPES: &[&str] = &["integer", "string", "boolean"];

/// `TRAN-071`: every POST request carries an `MCP-Protocol-Version` header.
///
/// Deliberately not the `2025-11-25` check of the same purpose: that one skips
/// everything up to the negotiated `initialize` result, so with the handshake
/// gone it would pass every trace without inspecting a single request.
pub(in crate::checks) fn protocol_version_header_present(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for post in posts(context) {
        if post.is_request() && !post.headers.contains_key("mcp-protocol-version") {
            sink.push(
                Some(post.seq),
                "client POST lacks the MCP-Protocol-Version header".to_owned(),
            );
        }
    }
}

/// `TRAN-072`: the header's value matches the body's `_meta` protocol version.
pub(in crate::checks) fn protocol_version_header_matches_body(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for post in posts(context) {
        if !post.is_request() {
            continue;
        }
        // Absence is TRAN-071's finding, not a mismatch; a body that states no
        // version is BASE-030's.
        let (Some(header), Some(body)) = (
            post.headers.get("mcp-protocol-version"),
            post.body_protocol_version(),
        ) else {
            continue;
        };
        if header != body {
            sink.push(
                Some(post.seq),
                format!(
                    "MCP-Protocol-Version header is {header:?} but the body's \
                     `_meta` protocol version is {body:?}"
                ),
            );
        }
    }
}

/// `TRAN-058`: the standard request metadata headers accompany each POST request.
///
/// The Standard Request Headers table defines each header *by its source field*
/// (`Mcp-Method` from `method`, `Mcp-Name` from `params.name` or `params.uri`),
/// so a header carrying something else is not the required header — the same
/// reading applied to both, and the reason a mismatch is reported here rather
/// than only as the server-side failure to reject it (TRAN-097/TRAN-100).
pub(in crate::checks) fn request_metadata_headers(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let designated = designations_by_tool(context);
    for post in posts(context) {
        if !post.is_request() {
            continue;
        }
        for mirror in mirrors(&post, &designated) {
            if mirror.header.starts_with("mcp-param-") {
                continue; // custom headers are TRAN-079's
            }
            let Some(sent) = post.headers.get(&mirror.header) else {
                sink.push(
                    Some(post.seq),
                    format!(
                        "POST for `{}` lacks the required `{}` header",
                        post.method().unwrap_or_default(),
                        mirror.label
                    ),
                );
                continue;
            };
            if compare(sent, &mirror.value) == Match::Mismatch {
                sink.push(
                    Some(post.seq),
                    format!(
                        "`{}` header is {sent:?} but `{}` is {:?}",
                        mirror.label, mirror.source, mirror.value
                    ),
                );
            }
        }
    }
}

/// The headers of `post` whose values the Base64 sentinel may carry.
fn encodable_headers<'a>(post: &Post<'a>) -> impl Iterator<Item = (&'a String, &'a String)> {
    post.headers
        .iter()
        .filter(|(name, _)| *name == "mcp-name" || name.starts_with("mcp-param-"))
}

/// `TRAN-077`/`TRAN-086`/`TRAN-087`: a value that cannot be carried plainly is
/// carried Base64-encoded.
///
/// Deliberately narrower than "everything the Value Encoding section says": the
/// marker-case rule (TRAN-089) and the sentinel-pattern rule (TRAN-092) are
/// separate clauses with separate checks, because a requirement judged by a
/// check that bundles its neighbours' rules cannot report which rule it broke.
pub(in crate::checks) fn header_value_encoding(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for post in posts(context) {
        if !post.is_request() {
            continue;
        }
        for (name, value) in encodable_headers(&post) {
            // Already encoded, or miscased — the latter is TRAN-089's finding,
            // and reporting it here too would blame the wrong clause.
            if sentinel_payload(value).is_some() || is_miscased_sentinel(value) {
                continue;
            }
            if !header_safe(value) {
                sink.push(
                    Some(post.seq),
                    format!(
                        "header `{name}` carries {value:?} unencoded; a value that is not \
                         safely representable in ASCII must use the Base64 sentinel"
                    ),
                );
            }
        }
    }
}

/// `TRAN-089`: the sentinel markers appear exactly as shown, in lowercase.
pub(in crate::checks) fn sentinel_marker_case(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for post in posts(context) {
        if !post.is_request() {
            continue;
        }
        for (name, value) in encodable_headers(&post) {
            if is_miscased_sentinel(value) {
                sink.push(
                    Some(post.seq),
                    format!(
                        "header `{name}` carries {value:?}, whose Base64 sentinel markers \
                         are miscased; they are case-sensitive and must be lowercase"
                    ),
                );
            }
        }
    }
}

/// `TRAN-092`: a plain value matching the sentinel pattern is encoded too.
///
/// The trace shows this directly: the header repeats the body value byte for
/// byte, which encoding could never produce for a value already shaped like the
/// sentinel.
pub(in crate::checks) fn sentinel_pattern_encoded(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let designated = designations_by_tool(context);
    for post in posts(context) {
        if !post.is_request() {
            continue;
        }
        for mirror in mirrors(&post, &designated) {
            if !mirror.encodable {
                continue;
            }
            let Some(sent) = post.headers.get(&mirror.header) else {
                continue;
            };
            if compare(sent, &mirror.value) == Match::UnencodedSentinel {
                sink.push(
                    Some(post.seq),
                    format!(
                        "`{}` carries {sent:?} verbatim; a value matching the sentinel \
                         pattern must itself be Base64-encoded to stay unambiguous",
                        mirror.label
                    ),
                );
            }
        }
    }
}

/// `TRAN-079`: designated tool parameters are mirrored into `Mcp-Param-*`.
///
/// Judged against the request itself: a `tools/call` supplying an argument the
/// server designated must carry the matching header. The designation lives in
/// the tool definition, so this binds only where the trace also carried the tool
/// list that declared it.
pub(in crate::checks) fn x_mcp_header_mirrored(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let designated = designations_by_tool(context);
    if designated.is_empty() {
        return;
    }
    for post in posts(context) {
        if !post.is_request() {
            continue;
        }
        for mirror in mirrors(&post, &designated) {
            if mirror.header.starts_with("mcp-param-") && !post.headers.contains_key(&mirror.header)
            {
                sink.push(
                    Some(post.seq),
                    format!(
                        "`tools/call` supplies `{}`, which the tool designates for header \
                         `{}`, but the POST does not carry it",
                        mirror.source, mirror.label
                    ),
                );
            }
        }
    }
}

/// `TRAN-080`: an `x-mcp-header` annotation is usable as a header name.
///
/// All five constraints the clause states are judgeable from a tool definition
/// alone, and all five are checked: non-empty, field-name token syntax, no
/// control characters (which token syntax excludes outright), case-insensitive
/// uniqueness within one `inputSchema`, and primitive-typed properties only.
/// The neighbouring bullets about static reachability belong to TRAN-081 and
/// TRAN-082, which the registry excludes — judging those needs a schema engine,
/// not a session.
pub(in crate::checks) fn x_mcp_header_name_valid(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, tool) in tool_definitions(context) {
        let Some(schema) = tool.get("inputSchema") else {
            continue;
        };
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for designation in designations(schema) {
            let duplicate = !seen.insert(designation.name.to_ascii_lowercase());
            for reason in annotation_faults(&designation, duplicate) {
                sink.push(
                    Some(seq),
                    format!(
                        "property `{}` designates `x-mcp-header` {:?}, which {reason}",
                        designation.path.join("."),
                        designation.name
                    ),
                );
            }
        }
    }
}

/// Every way `designation` breaks TRAN-080, as finding-ready clauses.
fn annotation_faults(designation: &Designation, duplicate: bool) -> Vec<String> {
    let mut faults = Vec::new();
    if designation.name.is_empty() {
        faults.push("is empty".to_owned());
    } else if !designation
        .name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || TCHAR.contains(&byte))
    {
        faults.push("is not an HTTP field-name token (`1*tchar`, RFC 9110 §5.1)".to_owned());
    }
    if duplicate {
        faults.push(
            "repeats an earlier `x-mcp-header` in this `inputSchema`; the values must be \
             case-insensitively unique"
                .to_owned(),
        );
    }
    // An untyped property states nothing to judge; only a stated non-primitive
    // type is a violation, which is why `number` is caught and absence is not.
    if let Some(declared) = designation.declared_type.as_deref()
        && !PRIMITIVE_TYPES.contains(&declared)
    {
        faults.push(format!(
            "annotates a `{declared}` property; only integer, string and boolean \
             parameters may carry one"
        ));
    }
    faults
}
