// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The [`Registry`] container: loading, invariants, and the embedded data.
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
/// report order: base protocol, lifecycle, transports, the basic utilities (ping,
/// progress, cancellation), the capability-gated server feature areas, the
/// client-feature pages, then authorization.
const AREAS_2025_11_25: &[&str] = &[
    include_str!("../../registry/2025-11-25/base.json"),
    include_str!("../../registry/2025-11-25/lifecycle.json"),
    include_str!("../../registry/2025-11-25/transport.json"),
    include_str!("../../registry/2025-11-25/transport-streamable-http.json"),
    include_str!("../../registry/2025-11-25/ping.json"),
    include_str!("../../registry/2025-11-25/progress.json"),
    include_str!("../../registry/2025-11-25/cancellation.json"),
    include_str!("../../registry/2025-11-25/tools.json"),
    include_str!("../../registry/2025-11-25/resources.json"),
    include_str!("../../registry/2025-11-25/prompts.json"),
    include_str!("../../registry/2025-11-25/logging.json"),
    include_str!("../../registry/2025-11-25/completion.json"),
    include_str!("../../registry/2025-11-25/pagination.json"),
    include_str!("../../registry/2025-11-25/elicitation.json"),
    include_str!("../../registry/2025-11-25/elicitation-security.json"),
    include_str!("../../registry/2025-11-25/sampling.json"),
    include_str!("../../registry/2025-11-25/roots.json"),
    include_str!("../../registry/2025-11-25/authorization.json"),
    include_str!("../../registry/2025-11-25/authorization-tokens.json"),
    include_str!("../../registry/2025-11-25/authorization-security.json"),
];

/// The embedded per-area registry documents for protocol revision `2026-07-28`.
///
/// Every in-scope page of the published text is entered; the extraction method and
/// per-area decisions are recorded in
/// `docs/reports/registry-extraction-2026-07-28-inventory-2026-08-06.md`.
const AREAS_2026_07_28: &[&str] = &[
    include_str!("../../registry/2026-07-28/messages.json"),
    include_str!("../../registry/2026-07-28/statelessness.json"),
    include_str!("../../registry/2026-07-28/schema.json"),
    include_str!("../../registry/2026-07-28/meta.json"),
    include_str!("../../registry/2026-07-28/icons.json"),
    include_str!("../../registry/2026-07-28/patterns-index.json"),
    include_str!("../../registry/2026-07-28/transport-http-endpoint.json"),
    include_str!("../../registry/2026-07-28/transport-http-messages.json"),
    include_str!("../../registry/2026-07-28/transport-http-version.json"),
    include_str!("../../registry/2026-07-28/transport-http-headers.json"),
    include_str!("../../registry/2026-07-28/transport-http-validation.json"),
    include_str!("../../registry/2026-07-28/transport-http-compat.json"),
    include_str!("../../registry/2026-07-28/discover.json"),
    include_str!("../../registry/2026-07-28/versioning.json"),
    include_str!("../../registry/2026-07-28/transports-index.json"),
    include_str!("../../registry/2026-07-28/transport-stdio.json"),
    include_str!("../../registry/2026-07-28/mrtr-server.json"),
    include_str!("../../registry/2026-07-28/mrtr-client.json"),
    include_str!("../../registry/2026-07-28/subscriptions.json"),
    include_str!("../../registry/2026-07-28/cancellation.json"),
    include_str!("../../registry/2026-07-28/progress.json"),
    include_str!("../../registry/2026-07-28/caching.json"),
    include_str!("../../registry/2026-07-28/completion.json"),
    include_str!("../../registry/2026-07-28/pagination.json"),
    include_str!("../../registry/2026-07-28/logging.json"),
    include_str!("../../registry/2026-07-28/tools.json"),
    include_str!("../../registry/2026-07-28/resources.json"),
    include_str!("../../registry/2026-07-28/prompts.json"),
    include_str!("../../registry/2026-07-28/elicitation.json"),
    include_str!("../../registry/2026-07-28/elicitation-security.json"),
    include_str!("../../registry/2026-07-28/sampling.json"),
    include_str!("../../registry/2026-07-28/roots.json"),
    include_str!("../../registry/2026-07-28/authorization.json"),
    include_str!("../../registry/2026-07-28/authorization-tokens.json"),
    include_str!("../../registry/2026-07-28/authorization-discovery.json"),
    include_str!("../../registry/2026-07-28/authorization-security.json"),
];

/// The embedded extension registries for protocol revision `2026-07-28`: the
/// clauses of each official extension that attaches to it, reported beside the
/// core clauses and never scored with them (ADR-0023). Every entry names its
/// extension, which is how [`Registry`] tells them from the core areas above.
const EXTENSIONS_2026_07_28: &[&str] = &[
    include_str!("../../registry/2026-07-28/extensions/tasks.json"),
    include_str!("../../registry/2026-07-28/extensions/tasks-errors.json"),
    include_str!("../../registry/2026-07-28/extensions/skills.json"),
    include_str!("../../registry/2026-07-28/extensions/skills-entries.json"),
    include_str!("../../registry/2026-07-28/extensions/skills-hosts.json"),
    include_str!("../../registry/2026-07-28/extensions/skills-security.json"),
];

/// A complete requirement registry for one protocol revision.
///
/// Its core clauses ([`Registry::requirements`]) decide the verdict. The clauses
/// of the extensions in force at the revision
/// ([`Registry::extension_requirements`]) are held apart, so that no reader of
/// the core clauses — a report's totals, a coverage count, an exclusion ledger —
/// can count an extension's by accident. A document may list both kinds under
/// `requirements`; loading moves every entry that names an `extension` to the
/// extension list, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    revision: ProtocolRevision,
    requirements: Vec<Requirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<Requirement>,
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
    /// Loads the embedded registry for protocol revision `2025-11-25`.
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
        let registry = Self::from_parts(REVISION_2025_11_25, requirements);
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
    ///
    /// Entries naming an extension go to the extension list, in order.
    pub(super) fn from_parts(revision: ProtocolRevision, requirements: Vec<Requirement>) -> Self {
        let (extensions, requirements) = requirements
            .into_iter()
            .partition(|requirement| requirement.extension.is_some());
        Self {
            revision,
            requirements,
            extensions,
        }
    }

    /// Parses and validates a registry from a single JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::Parse`] for malformed JSON and
    /// [`RegistryError::Invalid`] when invariants fail: duplicate requirement IDs, an
    /// empty `checks` list, or an empty `exclusion`, `quote`, or `section`.
    ///
    /// An entry naming an `extension` is an extension clause wherever the document
    /// lists it, and one under `extensions` that names none is
    /// [`RegistryError::Invalid`]: that list holds only extension clauses.
    pub fn from_json(text: &str) -> Result<Self, RegistryError> {
        let document: Self = serde_json::from_str(text).map_err(RegistryError::Parse)?;
        if let Some(core) = document
            .extensions
            .iter()
            .find(|requirement| requirement.extension.is_none())
        {
            return Err(RegistryError::Invalid(format!(
                "{}: listed under `extensions` but names no extension",
                core.id
            )));
        }
        let mut entries = document.requirements;
        entries.extend(document.extensions);
        let registry = Self::from_parts(document.revision, entries);
        registry.validate()?;
        Ok(registry)
    }

    /// The protocol revision this registry describes.
    #[must_use]
    pub const fn revision(&self) -> ProtocolRevision {
        self.revision
    }

    /// The core requirements, in registry (= report) order: every clause the
    /// revision's own pages state, and nothing an extension states.
    #[must_use]
    pub fn requirements(&self) -> &[Requirement] {
        &self.requirements
    }

    /// The clauses of the extensions in force at this revision, in registry order,
    /// each naming its extension. Reported beside the core clauses and never
    /// counted with them (ADR-0023).
    ///
    /// ```
    /// use mcp_conformance_core::requirement::RegistrySet;
    ///
    /// let set = RegistrySet::builtin()?;
    /// let registry = set.registry("2026-07-28".parse()?).unwrap();
    /// assert!(registry.extension_requirements().iter().all(|r| r.extension.is_some()));
    /// assert!(registry.requirements().iter().all(|r| r.extension.is_none()));
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    #[must_use]
    pub fn extension_requirements(&self) -> &[Requirement] {
        &self.extensions
    }

    /// Looks up a requirement by ID, core or extension.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Requirement> {
        self.requirements
            .iter()
            .chain(&self.extensions)
            .find(|requirement| requirement.id.as_str() == id)
    }

    fn validate(&self) -> Result<(), RegistryError> {
        let all: Vec<Requirement> = self
            .requirements
            .iter()
            .chain(&self.extensions)
            .cloned()
            .collect();
        validate_requirements(&all)
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
/// [`Registry::builtin_2025_11_25`] and must stay exactly the `2025-11-25` data. This
/// one is the set's union, and only [`RegistrySet::builtin`](super::RegistrySet::builtin)
/// uses it.
pub(super) fn builtin_set_requirements() -> Result<Vec<Requirement>, RegistryError> {
    let mut requirements = builtin_requirements()?;
    for document in AREAS_2026_07_28.iter().chain(EXTENSIONS_2026_07_28) {
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
/// non-empty quote and section, a well-formed extension identifier where one is
/// named, and a non-empty `checks` list or `exclusion` reason.
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
        if let Some(extension) = &requirement.extension
            && !super::extension::is_identifier(extension)
        {
            return Err(RegistryError::Invalid(format!(
                "{id}: extension {extension:?} is not an extension identifier \
                 (`vendor-prefix/extension-name`)"
            )));
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
