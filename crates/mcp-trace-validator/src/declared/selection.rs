// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which revision(s) to judge a trace against, and why ([`select`]).
//!
//! The rule has one authority. A session that completed an `initialize`
//! handshake runs under the revision the server's result names — that is what
//! negotiation *means* — so it is judged under that one alone, whatever the
//! client proposed and whatever its later requests state. A negotiated
//! revision this build has no registry for is refused, not approximated by the
//! proposal: the session did not run under the proposal. Only a session with no
//! completed handshake is judged by what it declares elsewhere.

use std::fmt;
use std::str::FromStr as _;

use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Responses, collect_all};

/// How the revision a report judges against was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RevisionSource {
    /// The caller named it (`--revision`).
    Requested,
    /// The trace declared it: see [`declared_revisions`] for what counts.
    ///
    /// [`declared_revisions`]: super::declared_revisions
    Declared,
    /// The trace declared nothing, so the newest supported revision was used.
    Default,
}

impl fmt::Display for RevisionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Requested => "requested",
            Self::Declared => "declared by the trace",
            Self::Default => "the trace declares none; newest supported",
        })
    }
}

/// The revisions to judge a trace against, and why those.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Selection {
    /// Ascending; never empty.
    pub revisions: Vec<ProtocolRevision>,
    /// How they were chosen.
    pub source: RevisionSource,
    /// The revision the client proposed in `initialize`, when the server's
    /// result negotiated a different one — the one in
    /// [`Self::revisions`]. `None` when they agree, or there was no handshake.
    pub proposed: Option<ProtocolRevision>,
}

/// The trace declares only revisions no available registry describes.
///
/// Judging it anyway would report every clause the revisions disagree about as a
/// violation, against an implementation that may have violated nothing — so this
/// is an error the caller must resolve (a newer build, or an explicit revision),
/// never a silent fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnjudgeableRevisions {
    /// What the trace declared, ascending — the one revision the server's
    /// `initialize` result negotiated, when [`Self::negotiated`].
    pub declared: Vec<ProtocolRevision>,
    /// What the available registries describe, ascending.
    pub supported: Vec<ProtocolRevision>,
    /// Whether [`Self::declared`] is the revision a completed handshake
    /// negotiated, which the session then ran under whatever else it stated.
    pub negotiated: bool,
    /// What the client proposed in that handshake, when it was well-formed.
    pub proposed: Option<ProtocolRevision>,
}

impl fmt::Display for UnjudgeableRevisions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let join = |revisions: &[ProtocolRevision]| {
            revisions
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        if self.negotiated {
            write!(
                f,
                "the server's initialize result negotiated protocol revision {}",
                join(&self.declared)
            )?;
            if let Some(proposed) = self.proposed {
                write!(f, " (the client proposed {proposed})")?;
            }
            return write!(
                f,
                ", which no available registry describes (supported: {}); the session ran \
                 under it, so judging it against another revision would report that \
                 revision's rules as violations",
                join(&self.supported)
            );
        }
        write!(
            f,
            "the trace declares protocol revision(s) {}, which no available registry \
             describes (supported: {}); judging it against another revision would \
             report that revision's rules as violations",
            join(&self.declared),
            join(&self.supported)
        )
    }
}

impl std::error::Error for UnjudgeableRevisions {}

/// Chooses the revisions to judge `events` against, from those `supported`.
///
/// 1. A session that completed an `initialize` handshake runs under the revision
///    the server's result names — that is what negotiation means — and is
///    judged under that one alone. [`Selection::proposed`] carries the client's
///    proposal when it differed, for the caller to say so. A negotiated
///    revision no registry describes is an error, whatever the client proposed:
///    judging the session under the proposal would judge rules it did not run
///    under.
/// 2. Without a handshake, every declared revision that is supported is judged:
///    the per-request `_meta` and the `MCP-Protocol-Version` header, which a
///    `2026-07-28` session states on each request, and an `initialize` no
///    result answered.
/// 3. A trace declaring only unsupported revisions is an error.
/// 4. A trace declaring nothing is judged against the newest supported revision.
///
/// # Errors
///
/// [`UnjudgeableRevisions`] in cases 1 and 3, or when `supported` is empty.
pub fn select(
    supported: &[ProtocolRevision],
    events: &[TraceEvent],
) -> Result<Selection, UnjudgeableRevisions> {
    let mut supported_sorted = supported.to_vec();
    supported_sorted.sort_unstable();
    supported_sorted.dedup();
    if let Some(negotiated) = negotiated_revision(events) {
        if !supported_sorted.contains(&negotiated.negotiated) {
            return Err(UnjudgeableRevisions {
                declared: vec![negotiated.negotiated],
                supported: supported_sorted,
                negotiated: true,
                proposed: negotiated.proposed,
            });
        }
        return Ok(Selection {
            revisions: vec![negotiated.negotiated],
            source: RevisionSource::Declared,
            proposed: negotiated
                .proposed
                .filter(|proposed| *proposed != negotiated.negotiated),
        });
    }
    let declared = collect_all(events);
    let judgeable: Vec<ProtocolRevision> = declared
        .iter()
        .copied()
        .filter(|revision| supported_sorted.contains(revision))
        .collect();
    if !judgeable.is_empty() {
        return Ok(Selection {
            revisions: judgeable,
            source: RevisionSource::Declared,
            proposed: None,
        });
    }
    match (declared.is_empty(), supported_sorted.last()) {
        (true, Some(&newest)) => Ok(Selection {
            revisions: vec![newest],
            source: RevisionSource::Default,
            proposed: None,
        }),
        _ => Err(UnjudgeableRevisions {
            declared: declared.into_iter().collect(),
            supported: supported_sorted,
            negotiated: false,
            proposed: None,
        }),
    }
}

/// The revision a completed handshake negotiated: what the server's
/// `initialize` result names, with what the client proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Negotiated {
    /// The `initialize` result's `protocolVersion`.
    negotiated: ProtocolRevision,
    /// The `initialize` request's `protocolVersion`, when it is well-formed.
    proposed: Option<ProtocolRevision>,
}

/// The first `initialize` the server answered with a result naming a
/// well-formed revision, read the way the lifecycle tracker reads a handshake:
/// a client request with an id, answered by the first server result carrying
/// that id after it. An `initialize` answered with an error (an unsupported
/// version, which the client meets by proposing another) negotiated nothing,
/// and neither did one a protected resource refused before reading it — the
/// retry is the handshake. A result whose `protocolVersion` is missing or
/// malformed is `LIFE-006`'s finding, not a revision.
fn negotiated_revision(events: &[TraceEvent]) -> Option<Negotiated> {
    let responses = Responses::of(events);
    let refused = crate::context::refused_requests(events);
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.direction == Direction::ClientToServer)
        .filter(|(_, event)| !refused.contains(&event.seq))
        .filter(|(_, event)| {
            event.message_payload().is_some_and(|payload| {
                payload.get("method").and_then(Value::as_str) == Some("initialize")
                    && payload.get("id").is_some()
            })
        })
        .find_map(|(index, event)| {
            let result = responses.result_for(events, index)?;
            let negotiated = result
                .get("protocolVersion")
                .and_then(Value::as_str)
                .and_then(|version| ProtocolRevision::from_str(version).ok())?;
            let proposed = event
                .message_payload()
                .and_then(|payload| payload.get("params"))
                .and_then(|params| params.get("protocolVersion"))
                .and_then(Value::as_str)
                .and_then(|version| ProtocolRevision::from_str(version).ok());
            Some(Negotiated {
                negotiated,
                proposed,
            })
        })
}
