// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! basic/utilities/progress: the token a request opts in with, and the
//! notifications its receiver sends against it — in either direction.
//!
//! A party asks for progress by putting `progressToken` in a request's
//! `params._meta`; the receiver of that request answers with
//! `notifications/progress` carrying the token. At this revision either party
//! issues requests, so a token's *owner* is identified by the party that sent
//! the request as well as by its text, and a notification is matched against
//! the requests of the party it is addressed to. Tokens are compared as
//! canonical JSON text, so the string `"1"` and the integer `1` are different
//! tokens, as they are different JSON values.
//!
//! The four rules about a token's shape and sequence are here; the two about
//! its lifetime — that progress stops when the operation does, and that a task's
//! progress keeps the task's token — need one shared walk of the recording and
//! live in [`lifetime`].

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::trace::TraceEvent;
use serde_json::Value;

use super::super::FindingSink;
use super::{Party, answers, notification_params, request_id, session_start};
use crate::context::TraceContext;

mod lifetime;
#[cfg(test)]
mod tests;

pub(in crate::checks) use lifetime::{stops_after_completion, task_token_unchanged};

/// The progress notification.
const PROGRESS: &str = "notifications/progress";

/// A request's `_meta.progressToken`, when `event` is a request carrying one,
/// whichever party sent it.
fn requested_token(event: &TraceEvent) -> Option<&Value> {
    let payload = event.message_payload()?;
    request_id(payload)?;
    payload.get("params")?.get("_meta")?.get("progressToken")
}

/// A progress notification's `params`, when `event` is one, from either party.
fn progress_params(event: &TraceEvent) -> Option<&Value> {
    notification_params(event, PROGRESS)
}

/// `PROG-009`: a progress token is a string or an integer.
pub(in crate::checks) fn token_string_or_integer(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, _, _) in context.messages() {
        let Some(token) = requested_token(event) else {
            continue;
        };
        // The subject is a request opting into progress, from either party.
        sink.examined();
        if !(token.is_string() || token.is_i64() || token.is_u64()) {
            sink.push(
                Some(event.seq),
                format!(
                    "the {}'s request carries `_meta.progressToken` {token}, not a string or \
                     integer",
                    Party::of(event.direction).name()
                ),
            );
        }
    }
}

/// `PROG-010`: no two active requests of one party share a progress token.
///
/// "Active" is read from the request until its answer; a request the recording
/// never sees answered stays active to the end, because nothing shows it
/// finished. The two parties' tokens are separate namespaces — each party's
/// notifications travel the other way — so a token one party holds does not
/// bar the other from choosing it.
pub(in crate::checks) fn token_unique_among_active_requests(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let answers = answers(context);
    // Active requests carrying a token: (sender, token text) → the request's seq.
    let mut active: BTreeMap<(Party, String), u64> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        if let Some(request) = answers.get(&event.seq) {
            active.retain(|_, held| held != request);
            continue;
        }
        let Some(token) = requested_token(event) else {
            continue;
        };
        // The subject is a request opting into progress.
        sink.examined();
        let party = Party::of(event.direction);
        let key = (party, to_canonical_string(token));
        if let Some(earlier) = active.get(&key) {
            sink.push(
                Some(event.seq),
                format!(
                    "progress token {token} is already held by the {}'s request at seq \
                     {earlier}, which has not been answered; tokens must be unique across \
                     a party's active requests",
                    party.name()
                ),
            );
        }
        active.insert(key, event.seq);
    }
}

/// `PROG-011`: each notification's `progress` exceeds the last for its token.
///
/// The sequence restarts when a new request takes the token up: a token may
/// be reused once its request is done, and the new operation's progress owes
/// nothing to the old one's. A `progress` that is not a number is the wire
/// schema's to report, and is skipped here.
pub(in crate::checks) fn value_strictly_increases(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // (party that sent the request the token belongs to, token text) → the
    // last `progress` sent against it.
    let mut last: BTreeMap<(Party, String), f64> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        if let Some(token) = requested_token(event) {
            last.remove(&(Party::of(event.direction), to_canonical_string(token)));
            continue;
        }
        let Some(params) = progress_params(event) else {
            continue;
        };
        let (Some(token), Some(progress)) = (
            params.get("progressToken"),
            params.get("progress").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let key = (
            Party::addressed_by(event.direction),
            to_canonical_string(token),
        );
        if let Some(previous) = last.insert(key, progress) {
            // The subject is a notification with a predecessor to exceed.
            sink.examined();
            if progress <= previous {
                sink.push(
                    Some(event.seq),
                    format!(
                        "`progress` for token {token} went from {previous} to {progress}; it \
                         must increase with each notification"
                    ),
                );
            }
        }
    }
}

/// `PROG-013` (the "provided in an active request" half): a progress
/// notification names a token some request of the party it is addressed to
/// actually provided.
///
/// Judged only when the recording holds the session's `initialize` request:
/// a `2025-11-25` session begins with it (LIFE-001), so from then on every
/// request the session issued is in the recording, and a token no such
/// request carried was never provided. Without the handshake the capture may
/// have begun mid-session and the token may predate it, so nothing is judged —
/// the same abstention `cancellation.references-issued-request` makes, and the
/// witness the stateless revision lacks (PROG-005 judges only the lifetime
/// half there). A token provided by the *other* party does not count: the
/// client's tokens name the client's requests, and a server reporting progress
/// against one of its own is reporting against nothing the client asked for.
pub(in crate::checks) fn token_issued_in_session(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let Some(start) = session_start(context) else {
        return;
    };
    // (sender, token text) of every request that opted into progress so far.
    let mut issued: BTreeSet<(Party, String)> = BTreeSet::new();
    for (event, _, _) in context.messages() {
        if let Some(token) = requested_token(event) {
            issued.insert((Party::of(event.direction), to_canonical_string(token)));
            continue;
        }
        let Some(token) = progress_params(event).and_then(|params| params.get("progressToken"))
        else {
            continue;
        };
        if event.seq < start {
            continue; // Before the session began: not this session's traffic.
        }
        // The subject is a progress notification sent in a session whose start
        // the recording holds.
        sink.examined();
        let text = to_canonical_string(token);
        let reporter = Party::of(event.direction);
        let addressee = reporter.other();
        if issued.contains(&(addressee, text.clone())) {
            continue;
        }
        let aside = if issued.contains(&(reporter, text)) {
            format!(
                " (the {} provided it in a request of its own, which is not a request the \
                 {} can report progress on)",
                reporter.name(),
                reporter.name()
            )
        } else {
            String::new()
        };
        sink.push(
            Some(event.seq),
            format!(
                "progress for token {token}, which no request from the {} provided in this \
                 session{aside}; the session began with `initialize` at seq {start}, so every \
                 request it issued is in the recording",
                addressee.name()
            ),
        );
    }
}
