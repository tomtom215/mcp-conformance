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

use mcp_conformance_core::trace::Direction;
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
        if event.direction != Direction::ClientToServer {
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

/// `RES-022`: `resources/read` never answers with an empty `contents` array.
///
/// The specification forbids the shape outright and says why in the same breath:
/// "An empty array is ambiguous — it could mean the resource exists but has no
/// content, or that it doesn't exist at all." Because the objection is the
/// ambiguity, the shape alone is the violation, and no reading of the server's
/// intent is needed to report it.
pub(in crate::checks) fn read_contents_non_empty(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges_for("resources/read") {
        let Some(contents) = exchange
            .result
            .and_then(|result| result.get("contents"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        sink.examined();
        if contents.is_empty() {
            sink.push(
                Some(exchange.response.seq),
                "`resources/read` answered with an empty `contents` array, which is ambiguous \
                 between an empty resource and a missing one"
                    .to_owned(),
            );
        }
    }
}
