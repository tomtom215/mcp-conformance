// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! server/utilities/logging#error-handling at `2025-11-25`: the code a
//! `logging/setLevel` naming no real level carries.

use super::super::FindingSink;
use super::super::support::{Declaration, server_capability};
use super::{Answer, INVALID_PARAMS};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// RFC 5424's eight severities, which the schema's `LoggingLevel` enumerates
/// as the complete set.
const LEVELS: &[&str] = &[
    "debug",
    "info",
    "notice",
    "warning",
    "error",
    "critical",
    "alert",
    "emergency",
];

/// `LOG-003`: "Invalid log level: `-32602` (Invalid params)".
///
/// **Stimulus:** a `logging/setLevel` request whose `params.level` is present
/// and is not one of the eight level names, sent to a server whose
/// `initialize` result declared `logging`. The level is invalid by the schema
/// itself, so the failure case needs no knowledge of the server — the witness
/// `LOG-010` uses at `2026-07-28`, where the level rides `_meta` instead.
///
/// **Conforming answer:** error `-32602`.
///
/// **Not judged:** a server that did not declare `logging` (it owes the
/// method `-32601`, not this clause's code), a session whose handshake the
/// trace does not hold, a request with no `level` at all (a missing
/// parameter, which the clause does not name), and `-32603`, the clause's own
/// "configuration errors" case.
///
/// **Reported:** any other answer — a result, which is the server accepting a
/// level that does not exist, or another error code.
pub(in crate::checks) fn set_level_invalid_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    if !matches!(
        server_capability(context, &["logging"]),
        Declaration::Declared
    ) {
        return;
    }
    for exchange in context.exchanges_for("logging/setLevel") {
        let Some(level) = exchange.params.and_then(|params| params.get("level")) else {
            continue;
        };
        if level.as_str().is_some_and(|name| LEVELS.contains(&name)) {
            continue;
        }
        let answer = Answer::of(&exchange);
        if answer.is_another_failure() {
            continue;
        }
        // The subject is a request naming a level outside the eight.
        sink.examined();
        if !answer.is_error(INVALID_PARAMS) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`logging/setLevel` named level {level}, which is not one of the eight \
                     RFC 5424 levels, and drew {} rather than -32602",
                    answer.describe()
                ),
            );
        }
    }
}
