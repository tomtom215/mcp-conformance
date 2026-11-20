// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A header that carries its body value in another spelling is not a mismatch:
//! the Base64 sentinel (`TRAN-090`, `TRAN-091`, `TRAN-103`) and an integer
//! written another way (`TRAN-101`).
//!
//! These clauses bind what a server does *before* it compares, which no trace
//! shows. What a trace shows is the comparison's verdict, and one verdict
//! proves the step was skipped: a `HeaderMismatch` (`-32020`) answering a
//! request whose headers, once decoded as the page requires, all agree with
//! its body. The existing header checks already perform that comparison
//! ([`compare`], [`numerically_equal`]); this holds the server's answer
//! against it.
//!
//! **Stimulus.** A request sent by POST, paired with its headers without ambiguity
//! and answered in JSON-RPC, that gives the server no *other* ground for a
//! `HeaderMismatch` — every one of:
//!
//! - `MCP-Protocol-Version` is present and equals the body's `_meta` version;
//! - every header the body obliges ([`mirrors`]) is present;
//! - a `tools/call` names a tool the trace lists, so its `x-mcp-header`
//!   designations — the headers the server expects — are the ones compared;
//! - no `Mcp-Param-*` header is sent beyond those, so none arrives for a value
//!   the body does not carry (a stray `Mcp-Method` or `Mcp-Name` cannot occur
//!   beside an encoded or respelled header: both are mirrored wherever those
//!   are);
//! - each mirrored header equals its body value verbatim — and that value is
//!   one a header may carry unencoded — or is one of the two spellings below.
//!
//! and that uses exactly one of the two spellings:
//!
//! - **encoded** (`TRAN-090`/`TRAN-091`/`TRAN-103`): an `Mcp-Name` or
//!   `Mcp-Param-*` value in the sentinel form whose Base64 payload decodes to
//!   the body value;
//! - **respelled** (`TRAN-101`): an integer parameter's header naming the same
//!   number in other decimal text (`42.0` for `42`).
//!
//! A request using both is neither clause's subject: a rejection of it does
//! not say which comparison failed.
//!
//! **Conforming.** Any answer but `-32020` — a result, or any other error (a
//! resource that does not exist, a method the server lacks).
//!
//! **Fails / warns.** `-32020`: the server compared the header as sent.
//! `TRAN-101` is a SHOULD and warns.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::super::super::super::FindingSink;
use super::super::{
    Designation, Match, Post, compare, designations_by_tool, header_safe, mirrors,
    posts_by_message, tool_definitions,
};
use super::{HEADER_MISMATCH, answer_code, numerically_equal};
use crate::context::TraceContext;

/// Which alternative spellings a faultless request's mirrored headers use.
#[derive(Debug, Clone, Copy, Default)]
struct Spelling {
    /// Some header carries its value in the Base64 sentinel.
    encoded: bool,
    /// Some integer header names the body's number in other decimal text.
    respelled: bool,
}

/// How `post` spells its mirrored headers; `None` when it gives the server any
/// ground for `HeaderMismatch` that neither spelling explains.
fn spelling(
    post: &Post<'_>,
    designated: &BTreeMap<String, Vec<Designation>>,
    listed: &BTreeSet<&str>,
) -> Option<Spelling> {
    if !post.is_request() {
        return None;
    }
    let version = post.headers.get("mcp-protocol-version")?;
    if post.body_protocol_version() != Some(version.as_str()) {
        return None;
    }
    let tool = post
        .payload
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str);
    if post.method() == Some("tools/call") && !tool.is_some_and(|tool| listed.contains(tool)) {
        return None;
    }
    let mirrored = mirrors(post, designated);
    let mut found = Spelling::default();
    for mirror in &mirrored {
        let sent = post.headers.get(&mirror.header)?;
        match compare(sent, &mirror.value) {
            Match::Carried if *sent == mirror.value && header_safe(sent) => {}
            Match::Carried if mirror.encodable && *sent != mirror.value => found.encoded = true,
            Match::Mismatch if numerically_equal(mirror, sent) => found.respelled = true,
            _ => return None,
        }
    }
    let stray = post.headers.keys().any(|name| {
        name.starts_with("mcp-param-") && !mirrored.iter().any(|mirror| mirror.header == *name)
    });
    (!stray).then_some(found)
}

/// Reports every `HeaderMismatch` answering a faultless request whose spelling
/// is `subject`'s.
fn rejected_spelling(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    subject: fn(Spelling) -> bool,
    what: &str,
) {
    let designated = designations_by_tool(context);
    let listed: BTreeSet<&str> = tool_definitions(context)
        .filter_map(|(_, tool)| tool.get("name")?.as_str())
        .collect();
    let by_message = posts_by_message(context);
    for exchange in context.exchanges() {
        let Some(post) = by_message.get(&exchange.request.seq) else {
            continue;
        };
        if !spelling(post, &designated, &listed).is_some_and(subject) {
            continue;
        }
        sink.examined();
        if answer_code(exchange.response) == Some(HEADER_MISMATCH) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the POST at seq {} carried headers that all match its body once {what}, \
                     yet the server rejected it with {HEADER_MISMATCH} (HeaderMismatch)",
                    post.seq
                ),
            );
        }
    }
}

/// `TRAN-090` / `TRAN-091` / `TRAN-103`: a sentinel-encoded `Mcp-Name` or
/// `Mcp-Param-*` value is decoded before it is compared with the body.
pub(in crate::checks) fn encoded_header_decoded(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    rejected_spelling(
        context,
        sink,
        |found| found.encoded && !found.respelled,
        "the Base64 sentinel is decoded",
    );
}

/// `TRAN-101`: an integer parameter's header is compared with the body
/// numerically, not as text.
pub(in crate::checks) fn integer_header_compared_numerically(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    rejected_spelling(
        context,
        sink,
        |found| found.respelled && !found.encoded,
        "integers are compared numerically",
    );
}

#[cfg(test)]
mod tests;
