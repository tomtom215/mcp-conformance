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
    include_str!("../../registry/2025-11-25/transport-streamable-http.json"),
    include_str!("../../registry/2025-11-25/tools.json"),
    include_str!("../../registry/2025-11-25/resources.json"),
    include_str!("../../registry/2025-11-25/prompts.json"),
    include_str!("../../registry/2025-11-25/logging.json"),
    include_str!("../../registry/2025-11-25/completion.json"),
    include_str!("../../registry/2025-11-25/pagination.json"),
];

/// The embedded per-area registry documents for protocol revision `2026-07-28`.
///
/// Built area by area (roadmap M2.5): this list grows as each area's clauses are
/// curated and its checks land, so it is deliberately shorter than the revision's
/// clause inventory. The complete, quote-verified inventory is the backlog, recorded
/// in `docs/reports/registry-extraction-2026-07-28-inventory-2026-08-06.md`.
#[cfg(feature = "draft-2026-07-28")]
const AREAS_2026_07_28: &[&str] = &[
    include_str!("../../registry/2026-07-28/messages.json"),
    include_str!("../../registry/2026-07-28/statelessness.json"),
    include_str!("../../registry/2026-07-28/schema.json"),
    include_str!("../../registry/2026-07-28/meta.json"),
    include_str!("../../registry/2026-07-28/icons.json"),
    include_str!("../../registry/2026-07-28/transport-http-endpoint.json"),
    include_str!("../../registry/2026-07-28/transport-http-messages.json"),
    include_str!("../../registry/2026-07-28/transport-http-version.json"),
    include_str!("../../registry/2026-07-28/transport-http-headers.json"),
    include_str!("../../registry/2026-07-28/transport-http-compat.json"),
    include_str!("../../registry/2026-07-28/discover.json"),
    include_str!("../../registry/2026-07-28/versioning.json"),
    include_str!("../../registry/2026-07-28/transports-index.json"),
    include_str!("../../registry/2026-07-28/transport-stdio.json"),
    include_str!("../../registry/2026-07-28/mrtr-server.json"),
    include_str!("../../registry/2026-07-28/mrtr-client.json"),
    include_str!("../../registry/2026-07-28/subscriptions.json"),
    include_str!("../../registry/2026-07-28/caching.json"),
    include_str!("../../registry/2026-07-28/completion.json"),
    include_str!("../../registry/2026-07-28/pagination.json"),
    include_str!("../../registry/2026-07-28/logging.json"),
    include_str!("../../registry/2026-07-28/tools.json"),
    include_str!("../../registry/2026-07-28/resources.json"),
    include_str!("../../registry/2026-07-28/prompts.json"),
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
        let requirements = builtin_requirements()?;
        let registry = Self {
            revision: REVISION_2025_11_25,
            requirements,
        };
        registry.validate()?;
        Ok(registry)
    }

    /// Builds a registry from already-validated parts, skipping the invariant check.
    ///
    /// The caller must guarantee `requirements` satisfies the registry invariants
    /// ([`validate_requirements`]). The only caller is
    /// [`RegistrySet::registry`](super::RegistrySet::registry), which
    /// projects a revision-filtered subset of a set whose union those same invariants
    /// were checked over — and a subset of unique, well-formed entries is itself unique
    /// and well-formed, so re-validating would only repeat work already done.
    pub(super) const fn from_parts(
        revision: ProtocolRevision,
        requirements: Vec<Requirement>,
    ) -> Self {
        Self {
            revision,
            requirements,
        }
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
        validate_requirements(&self.requirements)
    }
}

/// Loads and merges the embedded `2025-11-25` per-area documents in report order,
/// checking each declares the expected revision. Shared by the single-revision
/// [`Registry::builtin_2025_11_25`] and the multi-revision
/// [`RegistrySet::builtin`](super::RegistrySet::builtin); neither validates here, so each
/// caller owns the invariant check appropriate to its container.
pub(super) fn builtin_requirements() -> Result<Vec<Requirement>, RegistryError> {
    let mut requirements = Vec::new();
    for document in AREAS_2025_11_25 {
        let area: Registry = serde_json::from_str(document).map_err(RegistryError::Parse)?;
        if area.revision != REVISION_2025_11_25 {
            return Err(RegistryError::Invalid(format!(
                "embedded registry file declares revision {}, expected 2025-11-25",
                area.revision
            )));
        }
        requirements.extend(area.requirements);
    }
    Ok(requirements)
}

/// The union across every revision the embedded set describes.
///
/// Distinct from [`builtin_requirements`] on purpose: that one feeds
/// [`Registry::builtin_2025_11_25`] and must stay exactly the `2025-11-25` data
/// whatever features are on. This one is the set's union, and only
/// [`RegistrySet::builtin`](super::RegistrySet::builtin) uses it. Keeping them apart is
/// what stops a feature flag from silently changing the single-revision registry.
pub(super) fn builtin_set_requirements() -> Result<Vec<Requirement>, RegistryError> {
    #[allow(unused_mut, reason = "grows only when the draft feature is enabled")]
    let mut requirements = builtin_requirements()?;
    #[cfg(feature = "draft-2026-07-28")]
    for document in AREAS_2026_07_28 {
        let area: Registry = serde_json::from_str(document).map_err(RegistryError::Parse)?;
        if area.revision != crate::revision::REVISION_2026_07_28 {
            return Err(RegistryError::Invalid(format!(
                "embedded registry file declares revision {}, expected 2026-07-28",
                area.revision
            )));
        }
        requirements.extend(area.requirements);
    }
    Ok(requirements)
}

/// The per-requirement registry invariants, checked over any sequence: unique IDs, a
/// non-empty quote and section, and a non-empty `checks` list or `exclusion` reason.
///
/// Factored out so the single-revision [`Registry`] and the multi-revision
/// [`RegistrySet`](super::RegistrySet) enforce one definition of "well-formed" rather
/// than two that could drift.
pub(super) fn validate_requirements(requirements: &[Requirement]) -> Result<(), RegistryError> {
    let mut seen = std::collections::HashSet::new();
    for requirement in requirements {
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

#[cfg(test)]
mod tests;
