// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Pagination's one new judgeable clause at `2026-07-28`: what an invalid cursor
//! must draw.
//!
//! The client-side opacity rule (PAGE-010) is unchanged from `2025-11-25` and
//! reuses `pagination.cursor-opacity`, which reads cursor *provenance* and does
//! not touch the handshake — checked before being reused, since a check that
//! consults `initialize` is inert at this revision.

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// JSON-RPC `Invalid params`, which an invalid cursor must draw.
const INVALID_PARAMS: i64 = -32602;

/// The list operations that paginate.
const PAGINATED: &[&str] = &[
    "resources/list",
    "resources/templates/list",
    "prompts/list",
    "tools/list",
];

/// `PAGE-011`: an invalid cursor draws `-32602`.
///
/// "Invalid" is witnessed the only way a recording can witness it: the client
/// presented a cursor that this session never issued as a `nextCursor` for that
/// method. A cursor that *was* issued may still have expired, and a trace cannot
/// tell — so those are not judged, and the check abstains rather than guessing.
///
/// Where this fires, PAGE-010 usually fires too, and that is not double
/// reporting: the client fabricated the cursor (its defect) and the server then
/// honoured it instead of rejecting it (the server's). Only the second is this
/// clause's.
pub(in crate::checks) fn invalid_cursor_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let mut issued: BTreeSet<(&str, &str)> = BTreeSet::new();
    // Issuances take effect after the result that carried them, so a cursor is
    // only "known" to requests that follow it — walking exchanges in order keeps
    // a server from being excused by a cursor it had not yet handed out.
    let mut exchanges: Vec<_> = context.exchanges().collect();
    exchanges.sort_by_key(|exchange| exchange.request.seq);
    for exchange in exchanges {
        if !PAGINATED.contains(&exchange.method) {
            continue;
        }
        let presented = exchange
            .params
            .and_then(|params| params.get("cursor"))
            .and_then(Value::as_str);
        if let Some(cursor) = presented
            && !issued.contains(&(exchange.method, cursor))
        {
            // The subject is a request presenting a cursor this session never
            // issued: no such request, and the clause is untested here.
            sink.examined();
            let code = exchange
                .response
                .message_payload()
                .and_then(|payload| payload.get("error"))
                .and_then(|error| error.get("code"))
                .and_then(Value::as_i64);
            if code != Some(INVALID_PARAMS) {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "`{}` presented the cursor {cursor:?}, which this session never issued, \
                         and the server answered with {} rather than {INVALID_PARAMS}",
                        exchange.method,
                        code.map_or_else(|| "a result".to_owned(), |code| format!("error {code}"))
                    ),
                );
            }
        }
        if let Some(next) = exchange
            .result
            .and_then(|result| result.get("nextCursor"))
            .and_then(Value::as_str)
        {
            issued.insert((exchange.method, next));
        }
    }
}
