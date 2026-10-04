// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The three feature-page clauses this revision added that no `2025-11-25`
//! check covers: deterministic tool ordering, the safe integer range for a
//! header-mirrored argument, and the empty `contents` array.
//!
//! Everything else on the tools, resources and prompts pages either reuses a
//! shipped check — each read to the bottom first, since a check that consults
//! the removed handshake is inert here — or carries an exclusion.

use std::collections::BTreeSet;

use mcp_conformance_core::trace::{Direction, TransportKind};
use serde_json::Value;

use super::super::FindingSink;
use super::transport::designations_by_tool;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The largest integer IEEE 754 double-precision represents exactly.
const SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// `TOOL-022`: `tools/list` returns tools in a deterministic order.
///
/// The clause qualifies itself — "the same ordering across requests when the
/// underlying set of tools has not changed" — and that qualifier is exactly what
/// makes it checkable: two results whose tool *sets* are equal must list them in
/// the same order. Where the sets differ the list did change, and the clause
/// says nothing, so nothing is reported.
pub(in crate::checks) fn deterministic_order(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut seen: Option<(u64, Vec<String>)> = None;
    for exchange in context.exchanges_for("tools/list") {
        let Some(names) = exchange
            .result
            .and_then(|result| result.get("tools"))
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
        else {
            continue;
        };
        if let Some((first_seq, first)) = &seen {
            let same_set: BTreeSet<&String> = first.iter().collect();
            let this_set: BTreeSet<&String> = names.iter().collect();
            if same_set != this_set {
                continue; // The set changed, so the clause says nothing here.
            }
            // The subject is a re-listing of an unchanged set: one `tools/list`
            // can neither agree nor disagree with itself.
            sink.examined();
            if *first != names {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "`tools/list` returned the same tools in a different order than the \
                         result at seq {first_seq}, though the set did not change"
                    ),
                );
            }
        } else {
            seen = Some((exchange.response.seq, names));
        }
    }
}

/// `TOOL-034`: a header-mirrored integer stays inside the IEEE 754 safe range.
///
/// Scoped to arguments at an `x-mcp-header`-annotated path, because that is what
/// the clause is about: the value has to survive a round trip through a header
/// and back through a double. An integer elsewhere in the arguments is the
/// tool's own business.
pub(in crate::checks) fn x_mcp_header_integer_range(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let designations = designations_by_tool(context);
    // Driven from the requests themselves rather than from answered exchanges:
    // the clause binds the value the *client* sent, and a call the server never
    // answered carries exactly the same out-of-range argument.
    for (event, _, _) in context.messages() {
        // Mirroring is a Streamable HTTP mechanism: "Clients using other
        // transports (e.g., stdio) MAY ignore `x-mcp-header` annotations
        // entirely", so on stdio no value is mirrored and none is bound.
        if event.direction != Direction::ClientToServer
            || event.transport != TransportKind::StreamableHttp
        {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if payload.get("method").and_then(Value::as_str) != Some("tools/call") {
            continue;
        }
        let Some(params) = payload.get("params") else {
            continue;
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(paths) = designations.get(name) else {
            continue;
        };
        for designation in paths {
            let mut value = params.get("arguments");
            for segment in &designation.path {
                value = value.and_then(|current| current.get(segment));
            }
            let Some(integer) = value.and_then(Value::as_i64) else {
                continue;
            };
            sink.examined();
            if !(-SAFE_INTEGER..=SAFE_INTEGER).contains(&integer) {
                sink.push(
                    Some(event.seq),
                    format!(
                        "the argument mirrored into `{}` is {integer}, outside the \
                         IEEE 754 safe integer range",
                        designation.name
                    ),
                );
            }
        }
    }
}

/// `RES-022`: "Servers MUST NOT return an empty `contents` array for a
/// non-existent resource."
///
/// The clause binds a *non-existent* resource, and its next sentence concedes
/// the other reading of the same shape: an empty array "could mean the resource
/// exists but has no content". So the shape alone convicts nothing — judged that
/// way until 2026-10-04, this reported servers for faithfully returning an
/// existing resource with no content. Whether a resource exists is the server's
/// own knowledge (the reason RES-019 is excluded), and the one place a trace
/// carries the server's word on it is a not-found answer: `-32602`, the code
/// this revision assigns, or the withdrawn `-32002`. An empty `contents` for a
/// URI the same session also answered as not found is the violation, and every
/// answer about such a URI the subject; any other empty `contents` is left
/// unjudged.
pub(in crate::checks) fn read_contents_non_empty(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let uri_of = |exchange: &crate::context::Exchange<'_>| {
        exchange
            .params
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let missing: BTreeSet<String> = context
        .exchanges_for("resources/read")
        .filter(|exchange| {
            let code = exchange
                .response
                .message_payload()
                .and_then(|payload| payload.get("error")?.get("code")?.as_i64());
            matches!(code, Some(-32602 | -32002))
        })
        .filter_map(|exchange| uri_of(&exchange))
        .collect();
    for exchange in context.exchanges_for("resources/read") {
        let Some(uri) = uri_of(&exchange).filter(|uri| missing.contains(uri)) else {
            continue;
        };
        // The subject is every answer about a resource the server has said is
        // missing — the not-found error itself being the conforming one.
        sink.examined();
        let empty = exchange
            .result
            .and_then(|result| result.get("contents"))
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty);
        if empty {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`resources/read` of {uri:?} answered with an empty `contents` array, \
                     though this session also answered that URI as not found; a missing \
                     resource must draw an error, not an ambiguous empty result"
                ),
            );
        }
    }
}
