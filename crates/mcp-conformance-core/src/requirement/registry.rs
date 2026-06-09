// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The [`Registry`] container: loading, invariants, and the embedded seed data.
//!
//! The embedded `2025-11-25` registry lives as one JSON document per requirement area
//! (`registry/2025-11-25/*.json`, each under the file-size cap and reviewable in
//! isolation); [`Registry::builtin_2025_11_25`] merges them in report order and
//! validates the result as a whole, so cross-file invariants (duplicate IDs above all)
//! still hold. External registries remain single documents via [`Registry::from_json`].

use core::fmt;

use serde::{Deserialize, Serialize};

use super::{Requirement, Verification};
use crate::revision::{ProtocolRevision, REVISION_2025_11_25};

/// The embedded per-area registry documents for protocol revision `2025-11-25`, in
/// report order: base protocol, lifecycle, transports, then the capability-gated
/// feature areas.
const AREAS_2025_11_25: &[&str] = &[
    include_str!("../../registry/2025-11-25/base.json"),
    include_str!("../../registry/2025-11-25/lifecycle.json"),
    include_str!("../../registry/2025-11-25/transport.json"),
    include_str!("../../registry/2025-11-25/tools.json"),
    include_str!("../../registry/2025-11-25/resources.json"),
    include_str!("../../registry/2025-11-25/prompts.json"),
    include_str!("../../registry/2025-11-25/logging.json"),
    include_str!("../../registry/2025-11-25/completion.json"),
    include_str!("../../registry/2025-11-25/pagination.json"),
];

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
    /// Returns [`RegistryError`] if any embedded document fails to parse, declares the
    /// wrong revision, or the merged registry fails validation — all of which would be
    /// defects in this crate; the error path exists so that no caller ever needs a
    /// panicking variant.
    ///
    /// # Example
    ///
    /// ```
    /// use mcp_conformance_core::requirement::Registry;
    ///
    /// let registry = Registry::builtin_2025_11_25()?;
    /// assert_eq!(registry.revision().to_string(), "2025-11-25");
    /// # Ok::<(), mcp_conformance_core::requirement::RegistryError>(())
    /// ```
    pub fn builtin_2025_11_25() -> Result<Self, RegistryError> {
        let mut requirements = Vec::new();
        for document in AREAS_2025_11_25 {
            let area: Self = serde_json::from_str(document).map_err(RegistryError::Parse)?;
            if area.revision != REVISION_2025_11_25 {
                return Err(RegistryError::Invalid(format!(
                    "embedded registry file declares revision {}, expected 2025-11-25",
                    area.revision
                )));
            }
            requirements.extend(area.requirements);
        }
        let registry = Self {
            revision: REVISION_2025_11_25,
            requirements,
        };
        registry.validate()?;
        Ok(registry)
    }

    /// Parses and validates a registry from a single JSON document.
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
    fn builtin_registry_areas_merge_in_report_order() {
        // Requirements arrive grouped by area, ordinals ascending within each group —
        // the order reports render in, pinned against accidental file reshuffling.
        let registry = Registry::builtin_2025_11_25().unwrap();
        let ids: Vec<&str> = registry
            .requirements()
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect();
        let mut sorted_by_area_order = ids.clone();
        let area_rank = |id: &str| {
            AREA_ORDER
                .iter()
                .position(|area| id.starts_with(area))
                .unwrap_or(usize::MAX)
        };
        sorted_by_area_order.sort_by(|a, b| area_rank(a).cmp(&area_rank(b)).then(a.cmp(b)));
        assert_eq!(ids, sorted_by_area_order);
        assert!(ids.contains(&"BASE-001"));
        assert!(ids.contains(&"TRAN-001"));
    }

    /// Area prefixes in their report order, for the order test above.
    const AREA_ORDER: &[&str] = &[
        "BASE-", "LIFE-", "TRAN-", "TOOL-", "RES-", "PROM-", "LOG-", "COMP-", "PAGE-",
    ];

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
    fn rejects_malformed_capability_gates_at_parse_time() {
        let json = r#"{
            "revision": "2025-11-25",
            "requirements": [
                {"id": "TOOL-001", "level": "MUST", "actor": "server",
                 "capability": "tools",
                 "source": {"section": "server/tools#x", "quote": "MUST t"},
                 "checks": ["a"]}
            ]
        }"#;
        assert!(matches!(
            Registry::from_json(json),
            Err(RegistryError::Parse(_))
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
    fn error_display_and_source_carry_real_information() {
        use core::error::Error as _;

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
