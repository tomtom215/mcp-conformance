// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which protocol revision a server instance presents.
//!
//! `2026-07-28` is not a dialect of `2025-11-25`: SEP-2575 removes
//! `initialize` and sessions outright, so a server serving it answers
//! `server/discover` instead of a handshake, reads per-request `_meta` instead
//! of negotiated session state, and carries SEP-2549 caching hints on results
//! the older revision has no field for. Those are transport-level and
//! result-level differences at once, which is why one process serves one
//! revision rather than both — the choice is made when the server is built and
//! never re-decided per request.
//!
//! **The default is [`ServedRevision::V2025_11_25`] and stays that way.** The
//! conformance gate, the agreement baseline, the draft-readiness ratchet and
//! every golden report in the corpus pin the `2025-11-25` surface byte for
//! byte; the newer revision is opt-in precisely so none of those move when it
//! gains a feature.

use rmcp::model::ProtocolVersion;

/// The protocol revision an [`EverythingServer`](super::EverythingServer)
/// presents.
///
/// Marked `#[non_exhaustive]`: revisions arrive on the specification's
/// schedule, and adding the next one must not be a breaking change for
/// downstream matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServedRevision {
    /// `2025-11-25` — `initialize`, sessions, no caching hints. The default,
    /// and the revision the conformance registry judges.
    #[default]
    V2025_11_25,
    /// `2026-07-28` — stateless (SEP-2575): no `initialize`, no sessions,
    /// `server/discover` for capability advertisement, per-request `_meta`
    /// carrying the protocol version and client capabilities, and SEP-2549
    /// caching hints on cacheable results.
    V2026_07_28,
}

impl ServedRevision {
    /// The wire identifier this revision negotiates as.
    #[must_use]
    pub const fn protocol_version(self) -> ProtocolVersion {
        match self {
            Self::V2025_11_25 => ProtocolVersion::V_2025_11_25,
            Self::V2026_07_28 => ProtocolVersion::V_2026_07_28,
        }
    }

    /// Whether this revision is served without sessions (SEP-2575).
    ///
    /// Drives two transport settings at once — sessions off, per-request
    /// protocol metadata required — because SEP-2575 removed the one and
    /// introduced the other in the same stroke; a server with sessions off but
    /// metadata unenforced would accept requests that name no revision at all.
    #[must_use]
    pub const fn is_stateless(self) -> bool {
        matches!(self, Self::V2026_07_28)
    }

    /// Whether results this revision defines caching hints for should carry
    /// them (SEP-2549's `ttlMs` and `cacheScope`).
    ///
    /// Read from the *served* revision rather than from the negotiated version
    /// of the request in hand, and deliberately so: a `2026-07-28` request can
    /// reach a server serving `2025-11-25` — `cargo xtask draft-readiness`
    /// drives exactly that every run — and keying the hints off the request
    /// would change what that server answers, moving a committed ratchet as a
    /// side effect of adding a mode nobody enabled.
    #[must_use]
    pub const fn emits_caching_hints(self) -> bool {
        matches!(self, Self::V2026_07_28)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_revision_every_baseline_pins() {
        assert_eq!(ServedRevision::default(), ServedRevision::V2025_11_25);
        assert_eq!(
            ServedRevision::default().protocol_version(),
            ProtocolVersion::V_2025_11_25
        );
    }

    #[test]
    fn each_variant_negotiates_as_its_own_revision() {
        assert_eq!(
            ServedRevision::V2025_11_25.protocol_version(),
            ProtocolVersion::V_2025_11_25
        );
        assert_eq!(
            ServedRevision::V2026_07_28.protocol_version(),
            ProtocolVersion::V_2026_07_28
        );
    }

    #[test]
    fn only_the_newer_revision_is_stateless_and_hints_at_caching() {
        assert!(!ServedRevision::V2025_11_25.is_stateless());
        assert!(!ServedRevision::V2025_11_25.emits_caching_hints());
        assert!(ServedRevision::V2026_07_28.is_stateless());
        assert!(ServedRevision::V2026_07_28.emits_caching_hints());
    }
}
