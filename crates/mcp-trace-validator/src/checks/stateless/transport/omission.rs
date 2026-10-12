// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `x-mcp-header` behaviour table (`#server-behavior-for-custom-headers`):
//! which `Mcp-Param-*` headers a `tools/call` carries, and what a server may
//! make of their absence.
//!
//! | Scenario | Client | Server |
//! |---|---|---|
//! | value provided | include the header (TRAN-130) | validate it (TRAN-131) |
//! | value is `null` | omit the header (TRAN-132) | not expect it (TRAN-133) |
//! | not in arguments | omit the header (TRAN-134) | not expect it (TRAN-135) |
//! | value in body, header omitted | non-conforming | reject (TRAN-136) |
//!
//! The first row restates TRAN-079 and TRAN-097, whose checks back it. The
//! other three are judged here, each against the tool definition the trace
//! itself carried: a designation the recording never saw binds nothing.

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::super::FindingSink;
use super::{
    Designation, Match, Post, compare, designations_by_tool, header_safe, mirrors, posts,
    posts_by_message, sentinel_payload, value_at,
};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// `HeaderMismatch`.
const HEADER_MISMATCH: i64 = -32020;

/// The two table rows in which a designated parameter has no value. They state
/// the same obligations, but they are two rows — two scenarios a session can
/// show — so each is judged on its own and a finding names the row it is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoValue {
    /// "Parameter value is `null`".
    Null,
    /// "Parameter not in arguments".
    Absent,
}

impl NoValue {
    /// How a finding states the scenario.
    const fn describe(self) -> &'static str {
        match self {
            Self::Null => "`null`",
            Self::Absent => "absent from the arguments",
        }
    }
}

/// The designations of the tool a `tools/call` POST names whose argument is in
/// `scenario`: `null` in `params.arguments`, or not there at all.
fn without_value<'d>(
    post: &Post<'_>,
    designated: &'d BTreeMap<String, Vec<Designation>>,
    scenario: NoValue,
) -> Vec<&'d Designation> {
    if !post.is_request() || post.method() != Some("tools/call") {
        return Vec::new();
    }
    let params = post.payload.get("params");
    let Some(declared) = params
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .and_then(|tool| designated.get(tool))
    else {
        return Vec::new();
    };
    let arguments = params.and_then(|params| params.get("arguments"));
    declared
        .iter()
        .filter(|designation| {
            let found = match arguments.and_then(|args| value_at(args, &designation.path)) {
                None => NoValue::Absent,
                Some(Value::Null) => NoValue::Null,
                Some(_) => return false,
            };
            found == scenario
        })
        .collect()
}

/// `TRAN-132`: a designated parameter whose value is `null` has no header.
pub(in crate::checks) fn x_mcp_header_null_omitted(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    header_omitted(context, sink, NoValue::Null);
}

/// `TRAN-134`: a designated parameter absent from the arguments has no header.
pub(in crate::checks) fn x_mcp_header_absent_omitted(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    header_omitted(context, sink, NoValue::Absent);
}

/// The shared body of TRAN-132 and TRAN-134.
fn header_omitted(context: &TraceContext<'_>, sink: &mut FindingSink, scenario: NoValue) {
    let designated = designations_by_tool(context);
    if designated.is_empty() {
        return;
    }
    for post in posts(context) {
        for designation in without_value(&post, &designated, scenario) {
            // The subject is a designated parameter the call left without a
            // value; a call supplying every one never meets this rule.
            sink.examined();
            if let Some(sent) = post.headers.get(&designation.header) {
                sink.push(
                    Some(post.seq),
                    format!(
                        "`params.arguments.{}` is {}, yet the POST carries `Mcp-Param-{}: \
                         {sent}`; the header is omitted when no value is present",
                        designation.path.join("."),
                        scenario.describe(),
                        designation.name
                    ),
                );
            }
        }
    }
}

/// Whether nothing on `post` but an omitted, valueless parameter could justify
/// a `HeaderMismatch`: every header the body requires is present and carries
/// its value exactly, the protocol-version header agrees with `_meta`, and no
/// `Mcp-Param-*` header holds characters a header may not carry unencoded.
///
/// Deliberately strict. A value the server could have read either way — an
/// integer spelled `42.0`, a sentinel left unencoded — counts as a reason, so
/// the rejection is left unjudged rather than convicted on a reading.
fn nothing_else_to_reject(
    post: &Post<'_>,
    designated: &BTreeMap<String, Vec<Designation>>,
) -> bool {
    let mirrored = mirrors(post, designated).iter().all(|mirror| {
        post.headers
            .get(&mirror.header)
            .is_some_and(|sent| compare(sent, &mirror.value) == Match::Carried)
    });
    let version_agrees = post
        .headers
        .get("mcp-protocol-version")
        .is_some_and(|sent| Some(sent.as_str()) == post.body_protocol_version());
    let params_valid = post
        .headers
        .iter()
        .filter(|(name, _)| name.starts_with("mcp-param-"))
        .all(|(_, value)| sentinel_payload(value).is_some() || header_safe(value));
    mirrored && version_agrees && params_valid
}

/// `TRAN-133`: a server does not expect the header for a `null` parameter.
pub(in crate::checks) fn null_param_header_not_expected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    header_not_expected(context, sink, NoValue::Null);
}

/// `TRAN-135`: a server does not expect the header for an absent parameter.
pub(in crate::checks) fn absent_param_header_not_expected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    header_not_expected(context, sink, NoValue::Absent);
}

/// The shared body of TRAN-133 and TRAN-135.
///
/// "Expect" reaches the wire in one form: a `HeaderMismatch` answering a POST
/// that conformed to the table — the valueless parameter's header left out, as
/// the client must — and gave the server nothing else to reject. A POST leaving
/// out headers for both scenarios is judged under both rows: the trace cannot
/// say which omission the server objected to.
fn header_not_expected(context: &TraceContext<'_>, sink: &mut FindingSink, scenario: NoValue) {
    let designated = designations_by_tool(context);
    if designated.is_empty() {
        return;
    }
    let by_message = posts_by_message(context);
    for exchange in context.exchanges() {
        let Some(post) = by_message.get(&exchange.request.seq) else {
            continue;
        };
        let omitted: Vec<_> = without_value(post, &designated, scenario)
            .into_iter()
            .filter(|designation| !post.headers.contains_key(&designation.header))
            .collect();
        if omitted.is_empty() || !nothing_else_to_reject(post, &designated) {
            continue;
        }
        sink.examined();
        let code = exchange
            .response
            .message_payload()
            .and_then(|payload| payload.get("error")?.get("code")?.as_i64());
        if code == Some(HEADER_MISMATCH) {
            let names: Vec<String> = omitted
                .iter()
                .map(|designation| format!("`Mcp-Param-{}`", designation.name))
                .collect();
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the POST at seq {} omitted {} for a parameter {}, as the client must, and \
                     carried every other header matching its body; the server rejected it \
                     with {HEADER_MISMATCH} (HeaderMismatch)",
                    post.seq,
                    names.join(", "),
                    scenario.describe()
                ),
            );
        }
    }
}

/// `TRAN-136`: a designated value in the body without its header is rejected.
///
/// The `Mcp-Param-*` counterpart of TRAN-097's mismatch: the header the body
/// requires is not there to compare at all, which is the case that check
/// steps over. Whether the rejection took the right status and code is
/// TRAN-098's to judge; what is reported here is a result.
pub(in crate::checks) fn missing_param_header_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let designated = designations_by_tool(context);
    if designated.is_empty() {
        return;
    }
    let by_message = posts_by_message(context);
    for exchange in context.exchanges() {
        let Some(post) = by_message.get(&exchange.request.seq) else {
            continue;
        };
        for mirror in mirrors(post, &designated) {
            if !mirror.header.starts_with("mcp-param-") || post.headers.contains_key(&mirror.header)
            {
                continue; // standard headers are TRAN-058's; present ones TRAN-097's
            }
            // The subject is an answered POST missing a header its body requires.
            sink.examined();
            if exchange.result.is_some() {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "the POST at seq {} carried `{}` = {:?} with no `{}` header; the server \
                         answered it with a result instead of rejecting it",
                        post.seq, mirror.source, mirror.value, mirror.label
                    ),
                );
            }
        }
    }
}
