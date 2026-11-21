// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Fetches of well-known documents, set apart from the MCP traffic.
//!
//! An `http` event that carries a `path` records a request to a well-known
//! URI beside the MCP endpoint (a recorder keeps a path nowhere else), or the
//! answer to one. Neither is MCP traffic: the request names no session and no
//! protocol version, and the answer is a metadata document. Judged as MCP
//! they would convict a conforming server of half the transport clauses, so
//! the engine takes them out before anything reads the trace as a session
//! and hands them to the few checks that judge the documents themselves.
//!
//! [`engine::validate`](crate::engine::validate) does this itself. Code that
//! builds a [`TraceContext`](crate::context::TraceContext) directly does it
//! with [`split`]:
//!
//! ```
//! use mcp_trace_validator::{context::TraceContext, fetches, reader};
//!
//! let trace = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"GET","path":"/.well-known/oauth-protected-resource"}"#;
//! let events = reader::parse_trace(trace, &reader::Limits::default())?;
//! let (session, fetched) = fetches::split(&events);
//! assert!(session.is_empty());
//! let context = TraceContext::new(&session).with_fetches(fetched);
//! # let _ = context;
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a private module the rest of the crate
// reads; this follows the rustc lint.
#![allow(clippy::redundant_pub_crate)]

use std::borrow::Cow;

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent};
use serde_json::Value;

/// The fetches of well-known documents [`split`] found in a trace, for
/// [`TraceContext::with_fetches`](crate::context::TraceContext::with_fetches).
#[derive(Debug, Clone, Default)]
pub struct Fetches<'a>(pub(crate) Vec<Fetch<'a>>);

/// One request to a well-known URI, and the answer the trace pairs with it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fetch<'a> {
    /// The path asked for.
    pub path: &'a str,
    /// The answer, when the trace holds one.
    pub answer: Option<Answer<'a>>,
}

/// The answer to a [`Fetch`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Answer<'a> {
    /// The response event's `seq`.
    pub seq: u64,
    /// The response status.
    pub status: u16,
    /// The JSON document the response carried, when it carried one.
    pub document: Option<&'a Value>,
}

/// The path an `http` event carries, with the rest of what a fetch reads.
fn fetched(event: &TraceEvent) -> Option<(&str, Option<u16>, Option<&Value>)> {
    match &event.body {
        EventBody::Http {
            path: Some(path),
            status,
            document,
            ..
        } => Some((path, *status, document.as_ref())),
        _ => None,
    }
}

/// `events` without the fetches of well-known documents, and those fetches.
///
/// An answer is paired with the earliest unanswered request for the same
/// path: a recorder writes the path it asked for on the answer too, so only
/// two requests for one path can be confused, and they asked the same thing.
/// An answer no request awaits is dropped with the rest.
#[must_use]
pub fn split(events: &[TraceEvent]) -> (Cow<'_, [TraceEvent]>, Fetches<'_>) {
    if events.iter().all(|event| fetched(event).is_none()) {
        return (Cow::Borrowed(events), Fetches::default());
    }
    let mut fetches: Vec<Fetch<'_>> = Vec::new();
    for event in events {
        let Some((path, status, document)) = fetched(event) else {
            continue;
        };
        match (event.direction, status) {
            (Direction::ClientToServer, _) => fetches.push(Fetch { path, answer: None }),
            (Direction::ServerToClient, Some(status)) => {
                if let Some(fetch) = fetches
                    .iter_mut()
                    .find(|fetch| fetch.answer.is_none() && fetch.path == path)
                {
                    fetch.answer = Some(Answer {
                        seq: event.seq,
                        status,
                        document,
                    });
                }
            }
            _ => {}
        }
    }
    let rest = events
        .iter()
        .filter(|event| fetched(event).is_none())
        .cloned()
        .collect();
    (Cow::Owned(rest), Fetches(fetches))
}

#[cfg(test)]
mod tests;
