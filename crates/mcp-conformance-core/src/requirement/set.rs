// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Multi-revision registry sets: the union of requirements across protocol revisions.
//!
//! A single-revision [`Registry`] judges a trace against one specification revision. A
//! [`RegistrySet`] carries the *union* of requirements across several revisions — each
//! entry tagged with the optional `applies` range that bounds its lifetime — together
//! with the list of revisions the data describes. Projecting the set to a revision
//! ([`RegistrySet::registry`]) filters the union to the entries in force there and hands
//! back an ordinary [`Registry`], so the migration from one revision to the next is a
//! data change plus a projection, not a second engine
//! ([02-architecture.md](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/02-architecture.md)
//! §Protocol-revision strategy).
//!
//! The embedded [`RegistrySet::builtin`] currently describes the single shipped revision,
//! `2025-11-25`; the loader is built to serve more than one so that the `2026-07-28`
//! entries drop in as data behind the `draft-2026-07-28` feature the day the final text
//! ships (roadmap M2.5), with no change to the projection or judgment machinery.

use serde::{Deserialize, Serialize};

use crate::revision::{ProtocolRevision, REVISION_2025_11_25};

use super::registry::{builtin_requirements, validate_requirements};
use super::{Registry, RegistryError, Requirement};

/// A requirement registry spanning more than one protocol revision.
///
/// ```
/// use mcp_conformance_core::requirement::RegistrySet;
/// use mcp_conformance_core::revision::ProtocolRevision;
///
/// // A two-revision set: BASE-001 is present throughout, LIFE-009 only before 2026-07-28.
/// let set = RegistrySet::from_json(r#"{
///     "revisions": ["2025-11-25", "2026-07-28"],
///     "requirements": [
///         {"id": "BASE-001", "level": "MUST", "actor": "both",
///          "source": {"section": "basic#x", "quote": "MUST x"}, "checks": ["a"]},
///         {"id": "LIFE-009", "level": "MUST", "actor": "server",
///          "applies": {"removed": "2026-07-28"},
///          "source": {"section": "life#y", "quote": "MUST y"}, "checks": ["b"]}
///     ]
/// }"#)?;
///
/// let old = "2025-11-25".parse::<ProtocolRevision>()?;
/// let new = "2026-07-28".parse::<ProtocolRevision>()?;
/// assert_eq!(set.registry(old).unwrap().requirements().len(), 2); // both apply
/// assert_eq!(set.registry(new).unwrap().requirements().len(), 1); // LIFE-009 was removed
/// assert!(set.registry("2024-01-01".parse()?).is_none()); // not a revision the set describes
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySet {
    revisions: Vec<ProtocolRevision>,
    requirements: Vec<Requirement>,
}

impl RegistrySet {
    /// Loads the embedded seed set. Today it describes the single shipped revision
    /// (`2025-11-25`) from the same per-area documents [`Registry::builtin_2025_11_25`]
    /// merges; the type serves more than one revision so future entries are pure data.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if any embedded document fails to parse or the merged
    /// set fails validation — defects in this crate, surfaced rather than panicked.
    ///
    /// # Example
    ///
    /// ```
    /// use mcp_conformance_core::requirement::RegistrySet;
    ///
    /// let set = RegistrySet::builtin()?;
    /// assert_eq!(set.revisions(), ["2025-11-25".parse()?]);
    /// // Projecting the sole revision reconstructs the single-revision builtin.
    /// let projected = set.registry("2025-11-25".parse()?).unwrap();
    /// assert_eq!(projected, mcp_conformance_core::requirement::Registry::builtin_2025_11_25()?);
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub fn builtin() -> Result<Self, RegistryError> {
        let set = Self {
            revisions: vec![REVISION_2025_11_25],
            requirements: builtin_requirements()?,
        };
        set.validate()?;
        Ok(set)
    }

    /// Parses and validates a registry set from a single JSON document of the shape
    /// `{ "revisions": [...], "requirements": [...] }`.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Parse`] for malformed JSON; [`RegistryError::Invalid`] when the
    /// set declares no revisions, declares a revision twice, fails a per-requirement
    /// invariant (duplicate IDs, empty quote/section/checks/exclusion), or carries a
    /// requirement that applies to none of the declared revisions.
    pub fn from_json(text: &str) -> Result<Self, RegistryError> {
        let set: Self = serde_json::from_str(text).map_err(RegistryError::Parse)?;
        set.validate()?;
        Ok(set)
    }

    /// The protocol revisions this set describes, in declared order.
    #[must_use]
    pub fn revisions(&self) -> &[ProtocolRevision] {
        &self.revisions
    }

    /// The union of requirements across every described revision, in registry order.
    #[must_use]
    pub fn requirements(&self) -> &[Requirement] {
        &self.requirements
    }

    /// Projects the set to the single-revision [`Registry`] in force at `revision`.
    ///
    /// Returns `None` when `revision` is not one the set describes — distinguishing "this
    /// set says nothing about that revision" from a revision it describes that happens to
    /// have no applicable requirements (an empty but real [`Registry`]). The projected
    /// registry contains exactly the entries whose `applies` range admits `revision`, in
    /// the set's order.
    #[must_use]
    pub fn registry(&self, revision: ProtocolRevision) -> Option<Registry> {
        if !self.revisions.contains(&revision) {
            return None;
        }
        let requirements = self
            .requirements
            .iter()
            .filter(|requirement| requirement.applies_to(revision))
            .cloned()
            .collect();
        Some(Registry::from_parts(revision, requirements))
    }

    fn validate(&self) -> Result<(), RegistryError> {
        if self.revisions.is_empty() {
            return Err(RegistryError::Invalid(
                "registry set declares no revisions".to_owned(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for revision in &self.revisions {
            if !seen.insert(*revision) {
                return Err(RegistryError::Invalid(format!(
                    "duplicate revision {revision}"
                )));
            }
        }
        validate_requirements(&self.requirements)?;
        // A requirement that applies to no described revision is dead data — never
        // served by any projection — and is far likelier a wrong `applies` bound than
        // an intentional entry.
        for requirement in &self.requirements {
            if !self
                .revisions
                .iter()
                .any(|&revision| requirement.applies_to(revision))
            {
                return Err(RegistryError::Invalid(format!(
                    "{}: applies to none of the set's revisions",
                    requirement.id
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rev(text: &str) -> ProtocolRevision {
        text.parse().unwrap()
    }

    /// A two-revision set exercising the three applicability shapes: present throughout,
    /// removed at the boundary, introduced at the boundary.
    const TWO_REVISION: &str = r#"{
        "revisions": ["2025-11-25", "2026-07-28"],
        "requirements": [
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "basic#x", "quote": "MUST x"}, "checks": ["a"]},
            {"id": "LIFE-009", "level": "MUST", "actor": "server",
             "applies": {"removed": "2026-07-28"},
             "source": {"section": "life#y", "quote": "MUST y"}, "checks": ["b"]},
            {"id": "DISC-001", "level": "MUST", "actor": "server",
             "applies": {"introduced": "2026-07-28"},
             "source": {"section": "disc#z", "quote": "MUST z"}, "checks": ["c"]}
        ]
    }"#;

    #[test]
    fn builtin_set_describes_the_shipped_revision_and_projects_to_the_single_registry() {
        let set = RegistrySet::builtin().unwrap();
        assert_eq!(set.revisions(), [REVISION_2025_11_25]);
        let projected = set.registry(REVISION_2025_11_25).unwrap();
        // Projection of the sole revision reconstructs the canonical single-revision
        // builtin byte-for-byte — the multi-revision path is a superset, not a fork.
        assert_eq!(projected, Registry::builtin_2025_11_25().unwrap());
    }

    #[test]
    fn requirements_accessor_returns_the_whole_union_in_order() {
        // The union accessor exposes every entry across revisions, unfiltered and in
        // registry order — distinct from `registry(rev)`, which reads the field directly
        // and so would not exercise this method.
        let set = RegistrySet::from_json(TWO_REVISION).unwrap();
        let ids: Vec<&str> = set.requirements().iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["BASE-001", "LIFE-009", "DISC-001"]);
    }

    #[test]
    fn projection_filters_to_the_requirements_in_force_at_each_revision() {
        let set = RegistrySet::from_json(TWO_REVISION).unwrap();

        let old = set.registry(rev("2025-11-25")).unwrap();
        let old_ids: Vec<&str> = old.requirements().iter().map(|r| r.id.as_str()).collect();
        assert_eq!(old_ids, ["BASE-001", "LIFE-009"]); // DISC-001 not yet introduced

        let new = set.registry(rev("2026-07-28")).unwrap();
        let new_ids: Vec<&str> = new.requirements().iter().map(|r| r.id.as_str()).collect();
        assert_eq!(new_ids, ["BASE-001", "DISC-001"]); // LIFE-009 removed
        assert_eq!(new.revision(), rev("2026-07-28"));
    }

    #[test]
    fn unknown_revision_projects_to_none() {
        let set = RegistrySet::from_json(TWO_REVISION).unwrap();
        assert!(set.registry(rev("2024-01-01")).is_none());
        // A described revision with applicable entries is Some, never None — pins the
        // membership guard against returning None unconditionally.
        assert!(set.registry(rev("2025-11-25")).is_some());
    }

    #[test]
    fn rejects_a_set_with_no_revisions() {
        let json = r#"{"revisions": [], "requirements": []}"#;
        assert!(matches!(
            RegistrySet::from_json(json),
            Err(RegistryError::Invalid(reason)) if reason.contains("no revisions")
        ));
    }

    #[test]
    fn rejects_duplicate_revisions() {
        let json = r#"{"revisions": ["2025-11-25", "2025-11-25"], "requirements": []}"#;
        assert!(matches!(
            RegistrySet::from_json(json),
            Err(RegistryError::Invalid(reason)) if reason.contains("duplicate revision")
        ));
    }

    #[test]
    fn rejects_a_requirement_applying_to_no_described_revision() {
        // The clause was removed at 2025-11-25 but the set only describes 2025-11-25 and
        // later, so it is in force at no described revision: dead data.
        let json = r#"{
            "revisions": ["2025-11-25", "2026-07-28"],
            "requirements": [
                {"id": "GONE-001", "level": "MUST", "actor": "both",
                 "applies": {"removed": "2025-11-25"},
                 "source": {"section": "x#y", "quote": "MUST x"}, "checks": ["a"]}
            ]
        }"#;
        assert!(matches!(
            RegistrySet::from_json(json),
            Err(RegistryError::Invalid(reason)) if reason.contains("applies to none")
        ));
    }

    #[test]
    fn reuses_the_per_requirement_invariants() {
        // Duplicate IDs across the union are caught by the shared validator, just as in a
        // single-revision registry.
        let json = r#"{
            "revisions": ["2025-11-25"],
            "requirements": [
                {"id": "BASE-001", "level": "MUST", "actor": "both",
                 "source": {"section": "a#b", "quote": "MUST a"}, "checks": ["a"]},
                {"id": "BASE-001", "level": "MUST", "actor": "both",
                 "source": {"section": "c#d", "quote": "MUST c"}, "checks": ["b"]}
            ]
        }"#;
        assert!(matches!(
            RegistrySet::from_json(json),
            Err(RegistryError::Invalid(reason)) if reason.contains("duplicate requirement id")
        ));
    }

    #[test]
    fn serde_round_trips() {
        let set = RegistrySet::from_json(TWO_REVISION).unwrap();
        let json = serde_json::to_string(&set).unwrap();
        let back = RegistrySet::from_json(&json).unwrap();
        assert_eq!(back, set);
    }
}
