// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Caching hints: `ttlMs` and `cacheScope` on the results a client may reuse.
//!
//! Fourteen of the page's eighteen clauses carry exclusions, and they share one
//! shape: the page is mostly about what a *client* does with a hint, and a cache
//! hit is exactly the case where nothing reaches the wire. The freshness rules
//! are stated in elapsed time as well, which checks may not consult. What is
//! left — and what is judged here — is the server's side: the hints must be
//! there, the TTL must be a non-negative number, and a paginated list must not
//! change its scope halfway through.

use std::collections::HashMap;

use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The operations whose `complete` results must carry caching hints.
const CACHEABLE: &[&str] = &[
    "server/discover",
    "tools/list",
    "prompts/list",
    "resources/list",
    "resources/templates/list",
    "resources/read",
];

/// The `resultType` of a result that is cacheable at all.
const COMPLETE: &str = "complete";

/// `CACH-001`: cacheable results carry caching hints.
///
/// `ttlMs` is the hint required, and `cacheScope` is not: no clause on the page
/// makes the scope mandatory, while CACH-008 governs the TTL's value and CACH-006
/// treats an absent TTL as a legacy server. Demanding a `cacheScope` would be
/// inventing a rule the specification declines to state.
///
/// A retry's result is exempt. CACH-003 says a result produced through MRTR
/// "MUST NOT be cached", and the page's own treatment of `input_required`
/// results — "not cacheable and carry no caching hints" — is the principle:
/// uncacheable results need no hints. Without the exemption a server would be
/// reported for correctly withholding a freshness hint nobody may act on.
pub(in crate::checks) fn hints_on_cacheable_results(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges() {
        if !CACHEABLE.contains(&exchange.method) {
            continue;
        }
        let Some(result) = exchange.result else {
            continue;
        };
        if result.get("resultType").and_then(Value::as_str) != Some(COMPLETE) {
            continue;
        }
        let from_retry = exchange.params.is_some_and(|params| {
            params.get("inputResponses").is_some() || params.get("requestState").is_some()
        });
        if from_retry || result.get("ttlMs").is_some() {
            continue;
        }
        sink.push(
            Some(exchange.response.seq),
            format!(
                "the `complete` result of `{}` carries no `ttlMs` caching hint",
                exchange.method
            ),
        );
    }
}

/// `CACH-008`: a server's `ttlMs` is a number, and not a negative one.
///
/// Judged wherever a server result carries the field, not only on the six
/// cacheable operations: the clause binds the value a server *provides*, and a
/// negative TTL is no more permitted on a result that did not have to carry one.
pub(in crate::checks) fn ttl_non_negative(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        let Some(ttl) = event
            .message_payload()
            .and_then(|payload| payload.get("result"))
            .and_then(|result| result.get("ttlMs"))
        else {
            continue;
        };
        match ttl.as_i64() {
            Some(value) if value >= 0 => {}
            Some(value) => sink.push(
                Some(event.seq),
                format!("`ttlMs` is {value}; servers must provide a value that is >= 0"),
            ),
            None => sink.push(
                Some(event.seq),
                format!("`ttlMs` is {ttl}, which is not an integer number of milliseconds"),
            ),
        }
    }
}

/// `CACH-015` and `CACH-016`: every page of one list request shares a scope.
///
/// "A given list request" is the cursor chain, and the chain is followed
/// exactly: a request whose `cursor` equals a previous result's `nextCursor`
/// continues that page sequence, and a request with no cursor starts a new one.
/// Grouping by method instead would merge two independent `tools/list` calls,
/// which the clause does not bind together — a server may legitimately answer
/// them with different scopes.
pub(in crate::checks) fn page_scope_consistent(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // (method, cursor a continuation would present) → the chain it continues.
    let mut awaiting: HashMap<(&str, String), usize> = HashMap::new();
    // Chain → the scope its first page declared, and where.
    let mut scopes: Vec<(Option<String>, u64)> = Vec::new();
    for exchange in context.exchanges() {
        let Some(result) = exchange.result else {
            continue;
        };
        let scope = result.get("cacheScope").map(ToString::to_string);
        let cursor = exchange
            .params
            .and_then(|params| params.get("cursor"))
            .map(ToString::to_string);
        let chain = cursor
            .and_then(|cursor| awaiting.remove(&(exchange.method, cursor)))
            .unwrap_or_else(|| {
                scopes.push((scope.clone(), exchange.response.seq));
                scopes.len() - 1
            });
        let (first, first_seq) = &scopes[chain];
        if *first != scope {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "this `{}` page declares cacheScope {} while the page at seq {first_seq} \
                     in the same request declared {}",
                    exchange.method,
                    scope.as_deref().unwrap_or("none"),
                    first.as_deref().unwrap_or("none")
                ),
            );
        }
        if let Some(next) = result.get("nextCursor") {
            awaiting.insert((exchange.method, next.to_string()), chain);
        }
    }
}
