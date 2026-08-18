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

use rmcp::ErrorData as McpError;
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

    /// The `instructions` this revision's server offers a client.
    ///
    /// Two strings rather than one with a conditional tail, because they are
    /// prose read by a model deciding what to call: the `2025-11-25` text names
    /// `logging/setLevel`, and that method does not exist at `2026-07-28` —
    /// SEP-2577 moved the level onto each request's `_meta`. A shared string
    /// would have to name it for both or neither, and either way the server
    /// would be describing a surface it does not have. Nothing judges this
    /// field; it is accurate because a reference implementation's guidance
    /// being wrong is its own defect.
    #[must_use]
    pub const fn instructions(self) -> &'static str {
        match self {
            Self::V2025_11_25 => {
                "Reference server for MCP conformance testing: every advertised \
                 capability is implemented and exercised by the official suite. \
                 Tools include echo, add, and the suite's test_* contract; \
                 resources test://static-text, test://static-binary, and the \
                 test://template/{id}/data template; four test_* prompts; \
                 logging/setLevel; completion/complete."
            }
            Self::V2026_07_28 => {
                "Reference server for MCP conformance testing, serving the \
                 stateless 2026-07-28 surface: no initialize, no sessions, and \
                 every request carrying its own _meta. Probe server/discover \
                 for capabilities. Tools include echo, add, and the suite's \
                 test_* contract; resources test://static-text, \
                 test://static-binary, and the test://template/{id}/data \
                 template; four test_* prompts; completion/complete. Log level \
                 rides each request's _meta at this revision; there is no \
                 logging/setLevel."
            }
        }
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

    /// The error answering a `resources/read` for a URI this server does not
    /// serve.
    ///
    /// The code differs by revision because `2026-07-28` **withdrew** the one
    /// `2025-11-25` uses. `basic/index#error-codes` lists it among the codes
    /// "Implementations of this protocol version **MUST NOT** emit", and names
    /// the replacement in the same line:
    ///
    /// > `-32002` — resource not found (2025-11-25 and earlier; replaced by
    /// > `-32602`).
    ///
    /// So a server serving the newer revision answers `-32602` (Invalid
    /// params) — the URI it was handed names nothing — while one serving
    /// `2025-11-25` keeps `-32002`, which is correct there and is what the
    /// official suite's `resources-read-*` scenarios expect.
    ///
    /// `data.uri` rides both, because a client that asked for several
    /// resources needs to know which one is missing.
    #[must_use]
    pub fn resource_not_found(self, uri: &str) -> McpError {
        let data = Some(serde_json::json!({ "uri": uri }));
        if self.is_stateless() {
            McpError::invalid_params("resource not found", data)
        } else {
            McpError::resource_not_found("resource not found", data)
        }
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
    fn the_instructions_do_not_offer_a_method_the_revision_removed() {
        // A model reads this field to decide what to call. `logging/setLevel`
        // is gone at 2026-07-28 (SEP-2577 moved the level onto each request's
        // `_meta`), so naming it there would be the reference implementation
        // handing out a wrong answer.
        // Offered, in the list of what this server answers…
        assert!(
            ServedRevision::V2025_11_25
                .instructions()
                .contains("; logging/setLevel;")
        );
        // …and at the newer revision, named only to say it is gone. Denying it
        // beats omitting it: a client that knows the method from the older
        // revision is exactly the one that needs telling.
        let stateless = ServedRevision::V2026_07_28.instructions();
        assert!(!stateless.contains("; logging/setLevel;"));
        assert!(stateless.contains("there is no logging/setLevel"));
        assert!(
            stateless.contains("server/discover"),
            "and it names the probe that replaced the handshake"
        );
    }

    #[test]
    fn a_missing_resource_draws_the_code_its_revision_permits() {
        // -32002 is withdrawn at 2026-07-28 (`basic/index#error-codes` lists
        // it under "MUST NOT emit these codes" and names -32602 as the
        // replacement), so the same absence has to answer differently
        // depending on which revision this server is serving. Nothing but a
        // recording that actually asks for a missing resource can catch this,
        // which is why the enriched capture asks for one.
        assert_eq!(
            ServedRevision::V2025_11_25
                .resource_not_found("test://gone")
                .code,
            rmcp::model::ErrorCode::RESOURCE_NOT_FOUND
        );
        assert_eq!(
            ServedRevision::V2026_07_28
                .resource_not_found("test://gone")
                .code,
            rmcp::model::ErrorCode::INVALID_PARAMS
        );
        // Either way the client is told *which* URI was missing.
        assert_eq!(
            ServedRevision::V2026_07_28
                .resource_not_found("test://gone")
                .data,
            Some(serde_json::json!({ "uri": "test://gone" }))
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
