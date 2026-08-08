// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2026-07-28` revision.
//!
//! Referenced by the `2026-07-28` registry entries and, like every check, run
//! against whatever registry the caller projects — a check is a pure function of
//! a trace, not of a revision. They live together here because they arrived
//! together with that revision's first extracted areas, and because splitting
//! them out keeps the `2025-11-25` modules reviewable in isolation.

use mcp_conformance_core::trace::{Direction, EventBody};

use crate::context::TraceContext;

pub(super) mod envelope;
pub(super) mod meta;
pub(super) mod transport;

/// The HTTP status of the response that carried the message at `seq`.
///
/// Several clauses pair a JSON-RPC error code with an HTTP status ("`400 Bad
/// Request` and `-32020`"), so a check needs the status the message travelled
/// under. The capture format puts the response's `http` event *before* the
/// message(s) it framed — see `mcp_everything_server::tap::record_response`,
/// which records the status and then the body — so the search runs backwards
/// from the message and stops at the first server-sent status. One `http` event
/// can therefore answer for several messages, which is exactly right for an SSE
/// stream: every frame rode the same response.
///
/// A stdio trace carries no status at all, so callers must treat `None` as "no
/// evidence" rather than as a failure.
fn http_status_for(context: &TraceContext<'_>, seq: u64) -> Option<(u64, u16)> {
    context
        .events()
        .iter()
        .rev()
        .filter(|event| event.seq < seq && event.direction == Direction::ServerToClient)
        .find_map(|event| match &event.body {
            EventBody::Http {
                status: Some(status),
                ..
            } => Some((event.seq, *status)),
            _ => None,
        })
}
