// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Capability gates: which negotiated capability a requirement's applicability hangs on.
//!
//! Many `2025-11-25` clauses bind a party only when the corresponding capability was
//! declared during initialization — tools clauses bind servers that declared `tools`,
//! subscription clauses bind servers that declared `resources.subscribe`. A
//! [`CapabilityGate`] encodes that dependency as registry data: a dotted path whose
//! first segment names the declaring party and whose remainder indexes into that
//! party's declared capability object. Resolution semantics are fixed by
//! [ADR-0006](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0006-capability-gated-applicability.md):
//! requirements gated on undeclared capabilities are *not applicable*, never vacuously
//! passed.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The party whose declared capabilities a gate is resolved against.
///
/// Deliberately *not* `#[non_exhaustive]`: capability declarations have exactly two
/// surfaces in the protocol (the `initialize` request's `params.capabilities` and the
/// `initialize` result's `capabilities`), so downstream code may match exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityParty {
    /// Resolved against the server's declared capabilities (`initialize` result).
    Server,
    /// Resolved against the client's declared capabilities (`initialize` request).
    Client,
}

/// A validated capability path: `server.` or `client.` followed by the key path into
/// that party's declared capability object, e.g. `server.resources.subscribe`.
///
/// ```
/// use mcp_conformance_core::capability::{CapabilityGate, CapabilityParty};
/// use serde_json::json;
///
/// let gate: CapabilityGate = "server.resources.subscribe".parse()?;
/// assert_eq!(gate.party(), CapabilityParty::Server);
/// assert!(gate.is_declared(Some(&json!({"resources": {"subscribe": true}}))));
/// assert!(!gate.is_declared(Some(&json!({"resources": {}}))));
/// assert!(!gate.is_declared(None));
/// # Ok::<(), mcp_conformance_core::capability::ParseCapabilityGateError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CapabilityGate(String);

impl CapabilityGate {
    /// The gate as registry text, e.g. `server.tools`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Which party's declared capabilities this gate resolves against.
    #[must_use]
    pub fn party(&self) -> CapabilityParty {
        if self.0.starts_with("server.") {
            CapabilityParty::Server
        } else {
            // The constructor admits exactly two prefixes.
            CapabilityParty::Client
        }
    }

    /// The key path after the party segment, in order.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('.').skip(1)
    }

    /// Whether the gated capability was declared in the given capability object.
    ///
    /// Declared means: every path segment resolves, and the final value is neither
    /// `false` nor `null` — so `{"tools": {}}` declares `server.tools`, while an absent
    /// key or `{"subscribe": false}` does not. `None` (no capability surface observed
    /// in the trace) is never a declaration.
    #[must_use]
    pub fn is_declared(&self, capabilities: Option<&Value>) -> bool {
        let Some(mut current) = capabilities else {
            return false;
        };
        for segment in self.segments() {
            match current.get(segment) {
                Some(next) => current = next,
                None => return false,
            }
        }
        !(current.is_null() || matches!(current, Value::Bool(false)))
    }
}

impl fmt::Display for CapabilityGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Error produced when parsing a [`CapabilityGate`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseCapabilityGateError {
    /// The rejected input, for diagnostics.
    pub input: String,
}

impl fmt::Display for ParseCapabilityGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid capability gate {:?}: expected `server.<path>` or `client.<path>` with non-empty dot-separated segments",
            self.input
        )
    }
}

impl core::error::Error for ParseCapabilityGateError {}

impl FromStr for CapabilityGate {
    type Err = ParseCapabilityGateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut segments = s.split('.');
        let party_ok = matches!(segments.next(), Some("server" | "client"));
        let mut rest = segments.peekable();
        let path_ok = rest.peek().is_some();
        if party_ok && path_ok && rest.all(|segment| !segment.is_empty()) {
            Ok(Self(s.to_owned()))
        } else {
            Err(ParseCapabilityGateError {
                input: s.to_owned(),
            })
        }
    }
}

impl TryFrom<String> for CapabilityGate {
    type Error = ParseCapabilityGateError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<CapabilityGate> for String {
    fn from(gate: CapabilityGate) -> Self {
        gate.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_well_formed_gates() {
        for (text, party) in [
            ("server.tools", CapabilityParty::Server),
            ("server.resources.subscribe", CapabilityParty::Server),
            ("client.roots", CapabilityParty::Client),
            ("client.roots.listChanged", CapabilityParty::Client),
        ] {
            let gate: CapabilityGate = text.parse().unwrap();
            assert_eq!(gate.party(), party, "{text}");
            assert_eq!(gate.as_str(), text);
            assert_eq!(gate.to_string(), text);
        }
    }

    #[test]
    fn gate_segments_are_the_path_after_the_party() {
        let gate: CapabilityGate = "server.resources.subscribe".parse().unwrap();
        let segments: Vec<&str> = gate.segments().collect();
        assert_eq!(segments, ["resources", "subscribe"]);
    }

    #[test]
    fn rejects_malformed_gates() {
        for bad in [
            "",
            "server",
            "client",
            "server.",
            "server..tools",
            "server.tools.",
            "wizard.tools",
            ".tools",
            "Server.tools",
        ] {
            let error = bad.parse::<CapabilityGate>().unwrap_err();
            assert!(error.to_string().contains(&format!("{bad:?}")), "{bad:?}");
        }
    }

    #[test]
    fn declaration_requires_resolution_to_a_truthy_value() {
        let gate: CapabilityGate = "server.resources.subscribe".parse().unwrap();
        // Declared: path resolves to true / an object.
        assert!(gate.is_declared(Some(&json!({"resources": {"subscribe": true}}))));
        assert!(gate.is_declared(Some(&json!({"resources": {"subscribe": {}}}))));
        // Not declared: absent key, false, null, or no surface at all.
        assert!(!gate.is_declared(Some(&json!({"resources": {}}))));
        assert!(!gate.is_declared(Some(&json!({"resources": {"subscribe": false}}))));
        assert!(!gate.is_declared(Some(&json!({"resources": {"subscribe": null}}))));
        assert!(!gate.is_declared(Some(&json!({}))));
        assert!(!gate.is_declared(None));
    }

    #[test]
    fn one_segment_paths_treat_empty_objects_as_declared() {
        // `{"tools": {}}` is how real servers declare the tools capability.
        let gate: CapabilityGate = "server.tools".parse().unwrap();
        assert!(gate.is_declared(Some(&json!({"tools": {}}))));
        assert!(gate.is_declared(Some(&json!({"tools": {"listChanged": false}}))));
        assert!(!gate.is_declared(Some(&json!({"prompts": {}}))));
        // Non-object intermediate values cannot resolve further segments.
        let nested: CapabilityGate = "server.tools.listChanged".parse().unwrap();
        assert!(!nested.is_declared(Some(&json!({"tools": true}))));
        assert!(!nested.is_declared(Some(&json!({"tools": ["listChanged"]}))));
    }

    #[test]
    fn serde_round_trips_and_rejects_invalid() {
        let gate: CapabilityGate = serde_json::from_str(r#""server.logging""#).unwrap();
        assert_eq!(gate.as_str(), "server.logging");
        assert_eq!(serde_json::to_string(&gate).unwrap(), r#""server.logging""#);
        assert!(serde_json::from_str::<CapabilityGate>(r#""logging""#).is_err());
    }

    #[test]
    fn error_display_carries_the_input() {
        let error = "nope".parse::<CapabilityGate>().unwrap_err();
        assert!(error.to_string().contains("nope"), "{error}");
    }
}
