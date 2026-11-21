// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! server/utilities/pagination#error-handling at `2026-07-28`: the one cursor
//! a trace can show to be invalid there.

use super::super::FindingSink;
use super::{Answer, INVALID_PARAMS};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The list methods that take a `cursor`.
const PAGINATED_METHODS: [&str; 4] = [
    "resources/list",
    "resources/templates/list",
    "prompts/list",
    "tools/list",
];

/// `PAGE-011`: "Invalid cursors SHOULD result in an error with code -32602".
///
/// At `2025-11-25` a cursor the session never issued is an invalid one, and
/// `pagination.invalid-cursor-rejected` judges that. This revision takes the
/// witness away: its caching rules let a client re-present a cursor issued
/// before the recording began, and a server whose cursors are positions in a
/// keyset may honour any string at all. What neither can excuse is a cursor
/// that is **not a string** — the schema types `cursor` as one, so a number,
/// a boolean, an array or an object is invalid whatever scheme the server
/// uses and whenever it was "issued".
///
/// **Stimulus:** a paginated list request whose `params.cursor` is present,
/// not `null` (which reads as no cursor) and not a string.
///
/// **Conforming answer:** error `-32602`.
///
/// **Reported:** any other answer — a result, which is the server paging on
/// from a cursor it cannot have issued, or another error code. An error naming
/// another failure of the request is not judged (see the module).
pub(in crate::checks) fn malformed_cursor_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges() {
        if !PAGINATED_METHODS.contains(&exchange.method) {
            continue;
        }
        let Some(cursor) = exchange
            .params
            .and_then(|params| params.get("cursor"))
            .filter(|cursor| !cursor.is_null() && !cursor.is_string())
        else {
            continue;
        };
        let answer = Answer::of(&exchange);
        if answer.is_another_failure() {
            continue;
        }
        // The subject is a list request whose cursor is no string.
        sink.examined();
        if !answer.is_error(INVALID_PARAMS) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`{}` presented the cursor {cursor}, which is not a string and so no \
                     cursor the server issued, and drew {} rather than -32602",
                    exchange.method,
                    answer.describe()
                ),
            );
        }
    }
}
