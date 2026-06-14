// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The requirement registry: normative spec clauses as data.
//!
//! Every entry records one normative clause of an MCP specification revision — its RFC
//! 2119 level, the actor it binds, a verbatim quote with a section reference, an
//! optional capability gate (ADR-0006), and how this toolkit verifies it: either a list
//! of check IDs implemented by the validator, or a documented exclusion explaining why
//! the clause cannot be judged from a recorded trace. The check-or-exclusion shape
//! deliberately mirrors SEP-2484's traceability files, so registry entries and SEP
//! traceability are one format, not two.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::applies::AppliesRange;
use crate::capability::CapabilityGate;
use crate::revision::ProtocolRevision;

mod registry;
mod set;

pub use registry::{Registry, RegistryError};
pub use set::RegistrySet;

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

    /// The area prefix, e.g. `LIFE` for `LIFE-001`.
    #[must_use]
    pub fn area(&self) -> &str {
        self.0.split_once('-').map_or("", |(area, _)| area)
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
    /// The normative sentence, verbatim from the published (rendered)
    /// specification text, with one declared exception: clauses whose
    /// normative content is a bullet or numbered list are flattened onto one
    /// line, list items joined with `; ` after the introducing `:` — every
    /// fragment between those separators remains verbatim. Inline code spans
    /// keep their backticks.
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
    /// The protocol-revision range this clause is in force at (ADR-0006). Absent means
    /// every revision; a present range is the half-open `[introduced, removed)` interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies: Option<AppliesRange>,
    /// Source location and verbatim quote.
    pub source: SourceRef,
    /// Check coverage or documented exclusion.
    #[serde(flatten)]
    pub verification: Verification,
}

impl Requirement {
    /// Whether this clause is in force at `revision`.
    ///
    /// A requirement with no [`applies`](Self::applies) range applies to every revision;
    /// otherwise the range's half-open `[introduced, removed)` semantics decide.
    #[must_use]
    pub fn applies_to(&self, revision: ProtocolRevision) -> bool {
        self.applies
            .as_ref()
            .is_none_or(|range| range.contains(revision))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
    fn requirement_id_area_is_the_prefix() {
        let id: RequirementId = "TOOL-014".parse().unwrap();
        assert_eq!(id.area(), "TOOL");
        assert_eq!("LIFE-001".parse::<RequirementId>().unwrap().area(), "LIFE");
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
        let id: RequirementId = "LIFE-001".parse().unwrap();
        assert_eq!(id.to_string(), "LIFE-001");

        let id_error = "nope".parse::<RequirementId>().unwrap_err();
        assert!(id_error.to_string().contains("nope"), "{id_error}");
    }

    #[test]
    fn gated_requirements_round_trip_their_capability() {
        let json = r#"{"id": "TOOL-001", "level": "MUST", "actor": "server",
             "capability": "server.tools",
             "source": {"section": "server/tools#x", "quote": "MUST t"},
             "checks": ["tools.list-shape"]}"#;
        let requirement: Requirement = serde_json::from_str(json).unwrap();
        assert_eq!(
            requirement.capability.as_ref().map(CapabilityGate::as_str),
            Some("server.tools")
        );
        let back = serde_json::to_string(&requirement).unwrap();
        assert!(back.contains(r#""capability":"server.tools""#), "{back}");

        // Ungated entries omit the member entirely.
        let ungated: Requirement = serde_json::from_str(
            r#"{"id": "BASE-001", "level": "MUST", "actor": "both",
                "source": {"section": "basic#x", "quote": "MUST x"},
                "checks": ["a"]}"#,
        )
        .unwrap();
        assert!(ungated.capability.is_none());
        let back = serde_json::to_string(&ungated).unwrap();
        assert!(!back.contains("capability"), "{back}");
    }

    #[test]
    fn applies_to_defaults_to_every_revision_and_honors_a_range() {
        let rev = |text: &str| text.parse::<ProtocolRevision>().unwrap();

        // No `applies` member: in force at every revision.
        let ungated: Requirement = serde_json::from_str(
            r#"{"id": "BASE-001", "level": "MUST", "actor": "both",
                "source": {"section": "basic#x", "quote": "MUST x"},
                "checks": ["a"]}"#,
        )
        .unwrap();
        assert!(ungated.applies.is_none());
        assert!(ungated.applies_to(rev("2025-11-25")));
        assert!(ungated.applies_to(rev("2099-01-01")));

        // A `removed` bound: in force before it, gone at and after it.
        let deprecated: Requirement = serde_json::from_str(
            r#"{"id": "LOG-009", "level": "MUST", "actor": "server",
                "applies": {"removed": "2026-07-28"},
                "source": {"section": "logging#x", "quote": "MUST log"},
                "checks": ["a"]}"#,
        )
        .unwrap();
        assert!(deprecated.applies_to(rev("2025-11-25")));
        assert!(!deprecated.applies_to(rev("2026-07-28")));

        // The member round-trips.
        let back = serde_json::to_string(&deprecated).unwrap();
        assert!(
            back.contains(r#""applies":{"removed":"2026-07-28"}"#),
            "{back}"
        );
    }
}
