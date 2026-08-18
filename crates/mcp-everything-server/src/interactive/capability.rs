// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The client-capability gate the interactive tools share.
//!
//! Two things it gets right that a per-tool `if` did not.
//!
//! **Where the declaration is read.** [`RequestContext::client_capabilities`]
//! resolves the *request's* `_meta` first and falls back to session state only
//! when a session exists, so one call site is correct at both revisions —
//! whereas reading `peer.peer_info()` is correct only at `2025-11-25`. At
//! `2026-07-28` over stdio there is no handshake and no per-request peer to
//! synthesize one from, so `peer_info()` is `None` for every request and every
//! interactive tool would refuse a client that had declared the capability in
//! the envelope it was looking straight at.
//!
//! **What the refusal says.** SEP-2021 gave this exact situation an error:
//! `-32021`, carrying `data.requiredCapabilities` so the client learns what to
//! declare. It does not exist at `2025-11-25`, where the suite's scenarios
//! expect the older `invalid_request`, so the code is chosen by the served
//! revision rather than by which reads better.

use rmcp::RoleServer;
use rmcp::model::{ClientCapabilities, ElicitationCapability, ErrorData, SamplingCapability};
use rmcp::service::RequestContext;

use crate::server::ServedRevision;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

/// A client capability an interactive tool cannot proceed without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Required {
    /// `sampling/createMessage`.
    Sampling,
    /// `elicitation/create`, in either mode.
    Elicitation,
}

impl Required {
    /// This capability's name, as the client declares it.
    const fn name(self) -> &'static str {
        match self {
            Self::Sampling => "sampling",
            Self::Elicitation => "elicitation",
        }
    }

    /// Whether `declared` includes it.
    const fn declared_in(self, declared: &ClientCapabilities) -> bool {
        match self {
            Self::Sampling => declared.sampling.is_some(),
            Self::Elicitation => declared.elicitation.is_some(),
        }
    }

    /// This capability alone, as the `data.requiredCapabilities` of a `-32021`.
    ///
    /// A `ClientCapabilities` object rather than a list of names: that is what
    /// the `2026-07-28` schema types the field as, and the shape is the point
    /// — it is the same object the client would have had to send.
    fn as_capabilities(self) -> ClientCapabilities {
        let mut required = ClientCapabilities::default();
        match self {
            Self::Sampling => required.sampling = Some(SamplingCapability::default()),
            Self::Elicitation => required.elicitation = Some(ElicitationCapability::default()),
        }
        required
    }
}

/// `Ok(())` when the client declared `required`; the revision's refusal
/// otherwise.
pub(super) fn require(
    context: &RequestContext<RoleServer>,
    revision: ServedRevision,
    required: Required,
) -> Result<(), ErrorData> {
    let declared = context
        .client_capabilities()
        .is_some_and(|declared| required.declared_in(&declared));
    if declared {
        return Ok(());
    }
    Err(refusal(revision, required))
}

/// The error a server owes a client that asked for something it cannot answer.
fn refusal(revision: ServedRevision, required: Required) -> ErrorData {
    if revision.is_stateless() {
        // BASE-035: `-32021`, and `data.requiredCapabilities` naming what is
        // missing. The client has no session to renegotiate, so the error body
        // is the only place it can learn what to send next time.
        ErrorData::missing_required_client_capability(required.as_capabilities())
    } else {
        ErrorData::invalid_request(
            format!(
                "client does not support {0} (no {0} capability advertised)",
                required.name()
            ),
            None,
        )
    }
}
