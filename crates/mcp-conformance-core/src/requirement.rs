// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The requirement registry: normative spec clauses as data.
//!
//! Every entry records one normative clause of an MCP specification revision — its RFC
//! 2119 level, the actor it binds, a verbatim quote with a section reference, and how
//! this toolkit verifies it: either a list of check IDs implemented by the validator, or
//! a documented exclusion explaining why the clause cannot be judged from a recorded
//! trace. The check-or-exclusion shape deliberately mirrors SEP-2484's traceability
//! files, so registry entries and SEP traceability are one format, not two.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityGate;
use crate::revision::{ProtocolRevision, REVISION_2025_11_25};

/// The embedded seed registry for protocol revision `2025-11-25`.
const REGISTRY_2025_11_25: &str = include_str!("../registry/2025-11-25.json");

/// RFC 2119 requirement level of a normative clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Level {
    /// An absolute requirement.
    #[serde(rename = "MUST")]
    Must,
    /// An absolute prohibition.
    #[serde(rename = "MUST NOT")]
    MustNot,
    /// A strong recommendation; violations are reported as warnings.
    #[serde(rename = "SHOULD")]
    Should,
    /// A strong recommendation against; violations are reported as warnings.
    #[serde(rename = "SHOULD NOT")]
    ShouldNot,
    /// Truly optional behavior; tracked for coverage, never a violation.
    #[serde(rename = "MAY")]
    May,
}

impl Level {
    /// Whether violating this level is an error (fails a validation run) rather than a
    /// warning or informational note.
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Must | Self::MustNot)
    }

    /// The RFC 2119 keyword, exactly as it appears in registry JSON and spec quotes.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Must => "MUST",
            Self::MustNot => "MUST NOT",
            Self::Should => "SHOULD",
            Self::ShouldNot => "SHOULD NOT",
            Self::May => "MAY",
        }
    }
}

/// The protocol party a requirement binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Actor {
    /// Binds server behavior.
    Server,
    /// Binds client behavior.
    Client,
    /// Binds both parties.
    Both,
}

/// A stable requirement identifier: an uppercase area prefix and a three-digit ordinal,
/// e.g. `LIFE-001`. IDs are never reused, including after a requirement is withdrawn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequirementId(String);

impl RequirementId {
    /// The identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequirementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Error produced when parsing a [`RequirementId`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseRequirementIdError {
    /// The rejected input, for diagnostics.
    pub input: String,
}

impl fmt::Display for ParseRequirementIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid requirement id {:?}: expected AREA-NNN (uppercase ASCII area, three-digit ordinal)",
            self.input
        )
    }
}

impl core::error::Error for ParseRequirementIdError {}

impl FromStr for RequirementId {
    type Err = ParseRequirementIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s.split_once('-').is_some_and(|(area, ordinal)| {
            !area.is_empty()
                && area.bytes().all(|b| b.is_ascii_uppercase())
                && ordinal.len() == 3
                && ordinal.bytes().all(|b| b.is_ascii_digit())
        });
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(ParseRequirementIdError {
                input: s.to_owned(),
            })
        }
    }
}

impl TryFrom<String> for RequirementId {
    type Error = ParseRequirementIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<RequirementId> for String {
    fn from(id: RequirementId) -> Self {
        id.0
    }
}

/// Where a requirement's text lives in the specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SourceRef {
    /// Path-with-anchor under the specification site for the quoted clause, e.g.
    /// `basic/lifecycle#initialization`.
    pub section: String,
    /// The normative sentence, verbatim from the published specification text.
    pub quote: String,
}

/// How a requirement is verified — the SEP-2484 traceability alternative: concrete
/// checks, or a documented exclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Verification {
    /// Verified by the listed validator check IDs.
    Checks {
        /// IDs of validator checks covering this requirement (non-empty).
        checks: Vec<String>,
    },
    /// Not mechanically verifiable from a recorded trace; the reason is documented.
    Excluded {
        /// Why no trace-level check exists, and where the requirement *is* enforced
        /// or tested instead.
        exclusion: String,
    },
}

/// One normative clause of the specification, as registry data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Requirement {
    /// Stable identifier (`AREA-NNN`).
    pub id: RequirementId,
    /// RFC 2119 level.
    pub level: Level,
    /// The party the clause binds.
    pub actor: Actor,
    /// The negotiated capability this clause is gated on, when it binds only after
    /// declaration (ADR-0006). Ungated clauses apply to every session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<CapabilityGate>,
    /// Source location and verbatim quote.
    pub source: SourceRef,
    /// Check coverage or documented exclusion.
    #[serde(flatten)]
    pub verification: Verification,
}

/// A complete requirement registry for one protocol revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    revision: ProtocolRevision,
    requirements: Vec<Requirement>,
}

/// Error produced when loading or validating a [`Registry`].
#[derive(Debug)]
#[non_exhaustive]
pub enum RegistryError {
    /// The registry document was not valid JSON for the expected shape.
    Parse(serde_json::Error),
    /// The registry parsed but violates an invariant.
    Invalid(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "registry is not valid registry JSON: {error}"),
            Self::Invalid(reason) => write!(f, "registry violates an invariant: {reason}"),
        }
    }
}

impl core::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

impl Registry {
    /// Loads the embedded seed registry for protocol revision `2025-11-25`.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if the embedded document fails to parse or validate —
    /// which would be a defect in this crate; the error path exists so that no caller
    /// ever needs a panicking variant.
    pub fn builtin_2025_11_25() -> Result<Self, RegistryError> {
        let registry = Self::from_json(REGISTRY_2025_11_25)?;
        if registry.revision == REVISION_2025_11_25 {
            Ok(registry)
        } else {
            Err(RegistryError::Invalid(format!(
                "embedded registry declares revision {}, expected 2025-11-25",
                registry.revision
            )))
        }
    }

    /// Parses and validates a registry from JSON text.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::Parse`] for malformed JSON and
    /// [`RegistryError::Invalid`] when invariants fail: duplicate requirement IDs, an
    /// empty `checks` list, or an empty `exclusion`, `quote`, or `section`.
    pub fn from_json(text: &str) -> Result<Self, RegistryError> {
        let registry: Self = serde_json::from_str(text).map_err(RegistryError::Parse)?;
        registry.validate()?;
        Ok(registry)
    }

    /// The protocol revision this registry describes.
    #[must_use]
    pub const fn revision(&self) -> ProtocolRevision {
        self.revision
    }

    /// All requirements, in registry (= report) order.
    #[must_use]
    pub fn requirements(&self) -> &[Requirement] {
        &self.requirements
    }

    /// Looks up a requirement by ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Requirement> {
        self.requirements
            .iter()
            .find(|requirement| requirement.id.as_str() == id)
    }

    fn validate(&self) -> Result<(), RegistryError> {
        let mut seen = std::collections::HashSet::new();
        for requirement in &self.requirements {
            let id = requirement.id.as_str();
            if !seen.insert(id) {
                return Err(RegistryError::Invalid(format!(
                    "duplicate requirement id {id}"
                )));
            }
            if requirement.source.quote.trim().is_empty() {
                return Err(RegistryError::Invalid(format!("{id}: empty quote")));
            }
            if requirement.source.section.trim().is_empty() {
                return Err(RegistryError::Invalid(format!("{id}: empty section")));
            }
            match &requirement.verification {
                Verification::Checks { checks } => {
                    if checks.is_empty() {
                        return Err(RegistryError::Invalid(format!(
                            "{id}: checks list is empty — use an exclusion instead"
                        )));
                    }
                    if checks.iter().any(|check| check.trim().is_empty()) {
                        return Err(RegistryError::Invalid(format!("{id}: empty check id")));
                    }
                }
                Verification::Excluded { exclusion } => {
                    if exclusion.trim().is_empty() {
                        return Err(RegistryError::Invalid(format!(
                            "{id}: empty exclusion reason"
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_parses_and_validates() {
        let registry = Registry::builtin_2025_11_25().unwrap();
        assert_eq!(registry.revision(), REVISION_2025_11_25);
        assert!(registry.requirements().len() >= 16);
        assert!(registry.get("LIFE-001").is_some());
        assert!(registry.get("NOPE-999").is_none());
    }

    #[test]
    fn builtin_registry_quotes_are_normative() {
        // Every entry's quote must contain the keyword its level claims — a cheap
        // tripwire against paraphrased (non-verbatim) quotes sneaking in.
        let registry = Registry::builtin_2025_11_25().unwrap();
        for requirement in registry.requirements() {
            let quote = &requirement.source.quote;
            let keyword = requirement.level.keyword();
            assert!(
                quote.contains(keyword),
                "{}: quote lacks its level keyword {keyword}: {quote}",
                requirement.id
            );
        }
    }

    #[test]
    fn requirement_id_parsing() {
        assert!("LIFE-001".parse::<RequirementId>().is_ok());
        assert!("BASE-012".parse::<RequirementId>().is_ok());
        for bad in [
            "life-001",
            "LIFE-1",
            "LIFE-0001",
            "LIFE001",
            "-001",
            "LIFE-01a",
            "",
        ] {
            assert!(
                bad.parse::<RequirementId>().is_err(),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_ids() {
        let json = r#"{
            "revision": "2025-11-25",
            "requirements": [
                {"id": "BASE-001", "level": "MUST", "actor": "both",
                 "source": {"section": "basic#x", "quote": "MUST x"},
                 "checks": ["a"]},
                {"id": "BASE-001", "level": "MUST", "actor": "both",
                 "source": {"section": "basic#y", "quote": "MUST y"},
                 "checks": ["b"]}
            ]
        }"#;
        assert!(matches!(
            Registry::from_json(json),
            Err(RegistryError::Invalid(reason)) if reason.contains("duplicate")
        ));
    }

    #[test]
    fn rejects_empty_checks_and_exclusions() {
        let empty_checks = r#"{
            "revision": "2025-11-25",
            "requirements": [
                {"id": "BASE-001", "level": "MUST", "actor": "both",
                 "source": {"section": "basic#x", "quote": "MUST x"},
                 "checks": []}
            ]
        }"#;
        assert!(matches!(
            Registry::from_json(empty_checks),
            Err(RegistryError::Invalid(reason)) if reason.contains("empty")
        ));

        let empty_exclusion = r#"{
            "revision": "2025-11-25",
            "requirements": [
                {"id": "BASE-001", "level": "MUST", "actor": "both",
                 "source": {"section": "basic#x", "quote": "MUST x"},
                 "exclusion": "  "}
            ]
        }"#;
        assert!(matches!(
            Registry::from_json(empty_exclusion),
            Err(RegistryError::Invalid(reason)) if reason.contains("exclusion")
        ));
    }

    #[test]
    fn verification_serde_round_trips_both_arms() {
        let registry = Registry::builtin_2025_11_25().unwrap();
        let json = serde_json::to_string(&registry).unwrap();
        let back = Registry::from_json(&json).unwrap();
        assert_eq!(back, registry);
        assert!(
            registry
                .requirements()
                .iter()
                .any(|r| matches!(r.verification, Verification::Excluded { .. }))
        );
        assert!(
            registry
                .requirements()
                .iter()
                .any(|r| matches!(r.verification, Verification::Checks { .. }))
        );
    }

    #[test]
    fn level_keyword_and_severity_tables_are_exact() {
        let table = [
            (Level::Must, "MUST", true),
            (Level::MustNot, "MUST NOT", true),
            (Level::Should, "SHOULD", false),
            (Level::ShouldNot, "SHOULD NOT", false),
            (Level::May, "MAY", false),
        ];
        for (level, keyword, is_error) in table {
            assert_eq!(level.keyword(), keyword);
            assert_eq!(level.is_error(), is_error, "{keyword}");
        }
    }

    #[test]
    fn display_and_error_impls_carry_real_information() {
        use core::error::Error as _;

        let id: RequirementId = "LIFE-001".parse().unwrap();
        assert_eq!(id.to_string(), "LIFE-001");

        let id_error = "nope".parse::<RequirementId>().unwrap_err();
        assert!(id_error.to_string().contains("nope"), "{id_error}");

        let parse_error = Registry::from_json("{").unwrap_err();
        assert!(
            parse_error.to_string().contains("not valid"),
            "{parse_error}"
        );
        assert!(parse_error.source().is_some());

        let invalid = Registry::from_json(
            r#"{"revision":"2025-11-25","requirements":[
                {"id":"BASE-001","level":"MUST","actor":"both",
                 "source":{"section":"basic#x","quote":"MUST x"},"checks":[]}]}"#,
        )
        .unwrap_err();
        assert!(invalid.to_string().contains("invariant"), "{invalid}");
        assert!(invalid.source().is_none());
    }
}
