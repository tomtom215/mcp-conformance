// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Revision ranges: which protocol revisions a requirement is in force at.
//!
//! MCP clauses enter and leave the specification across dated revisions — a clause is
//! *introduced in* one revision and may be *removed in* a later one. An [`AppliesRange`]
//! encodes that lifetime as the half-open interval `[introduced, removed)`: a requirement
//! applies at every revision `r` with `introduced <= r < removed`. The bound semantics
//! mirror how the spec changelog speaks — a clause *introduced in* `2026-07-28` carries
//! `introduced = 2026-07-28`, and a clause *removed in* `2026-07-28` (the first revision
//! that no longer has it) carries `removed = 2026-07-28`, so it applies up to but not
//! including that revision.
//!
//! Either bound may be open: an absent `introduced` means "since the earliest revision",
//! an absent `removed` means "still current". The common case — a clause present in every
//! revision — is encoded by the *absence* of an `applies` member on the requirement
//! (see [`Requirement::applies_to`](crate::requirement::Requirement::applies_to)); this
//! type models only the constrained case, so a present range must constrain at least one
//! end. The slot itself was deferred until a second revision landed (ADR-0006) — with one
//! revision there is nothing for a range to discriminate.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::revision::ProtocolRevision;

/// The half-open revision range `[introduced, removed)` a requirement applies to.
///
/// ```
/// use mcp_conformance_core::applies::AppliesRange;
/// use mcp_conformance_core::revision::ProtocolRevision;
///
/// let from = "2025-11-25".parse::<ProtocolRevision>()?;
/// let until = "2026-07-28".parse::<ProtocolRevision>()?;
/// let range = AppliesRange::new(Some(from), Some(until))?;
///
/// assert!(range.contains(from)); // lower bound is inclusive
/// assert!(!range.contains(until)); // upper bound is exclusive
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawAppliesRange")]
pub struct AppliesRange {
    /// Inclusive lower bound; `None` means "since the earliest revision".
    #[serde(skip_serializing_if = "Option::is_none")]
    introduced: Option<ProtocolRevision>,
    /// Exclusive upper bound; `None` means "still current".
    #[serde(skip_serializing_if = "Option::is_none")]
    removed: Option<ProtocolRevision>,
}

impl AppliesRange {
    /// Builds a range from optional bounds.
    ///
    /// # Errors
    ///
    /// - [`AppliesRangeError::Empty`] when both bounds are absent — an unbounded
    ///   requirement carries no `applies` member at all, rather than an empty range, so
    ///   there is one canonical encoding of "applies everywhere".
    /// - [`AppliesRangeError::Inverted`] when `introduced` is not strictly before
    ///   `removed`; equal or backwards bounds describe an interval no revision satisfies.
    pub fn new(
        introduced: Option<ProtocolRevision>,
        removed: Option<ProtocolRevision>,
    ) -> Result<Self, AppliesRangeError> {
        match (introduced, removed) {
            (None, None) => return Err(AppliesRangeError::Empty),
            (Some(lo), Some(hi)) if lo >= hi => {
                return Err(AppliesRangeError::Inverted {
                    introduced: lo,
                    removed: hi,
                });
            }
            _ => {}
        }
        Ok(Self {
            introduced,
            removed,
        })
    }

    /// The inclusive lower bound, if the range has one.
    #[must_use]
    pub const fn introduced(&self) -> Option<ProtocolRevision> {
        self.introduced
    }

    /// The exclusive upper bound, if the range has one.
    #[must_use]
    pub const fn removed(&self) -> Option<ProtocolRevision> {
        self.removed
    }

    /// Whether `revision` lies in the half-open interval `[introduced, removed)`.
    ///
    /// An absent bound does not constrain that end: `introduced = None` admits every
    /// revision at or below `removed`, and `removed = None` admits every revision at or
    /// above `introduced`.
    #[must_use]
    pub fn contains(&self, revision: ProtocolRevision) -> bool {
        let at_or_after_introduced = self.introduced.is_none_or(|lo| revision >= lo);
        let before_removed = self.removed.is_none_or(|hi| revision < hi);
        at_or_after_introduced && before_removed
    }
}

/// The deserialization mirror: the wire shape with no invariants, validated on the way
/// into [`AppliesRange`] via [`TryFrom`]. `deny_unknown_fields` keeps a typo'd bound name
/// (`removes` for `removed`) a load error rather than a silently open upper bound.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAppliesRange {
    #[serde(default)]
    introduced: Option<ProtocolRevision>,
    #[serde(default)]
    removed: Option<ProtocolRevision>,
}

impl TryFrom<RawAppliesRange> for AppliesRange {
    type Error = AppliesRangeError;

    fn try_from(raw: RawAppliesRange) -> Result<Self, Self::Error> {
        Self::new(raw.introduced, raw.removed)
    }
}

/// Error produced when constructing or deserializing an [`AppliesRange`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AppliesRangeError {
    /// Both bounds were absent; an unbounded requirement omits the `applies` member.
    Empty,
    /// `introduced` was not strictly before `removed`.
    Inverted {
        /// The offending lower bound.
        introduced: ProtocolRevision,
        /// The offending upper bound.
        removed: ProtocolRevision,
    },
}

impl fmt::Display for AppliesRangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "empty applies range: at least one of `introduced` or `removed` is required \
                 (omit `applies` entirely for a requirement that applies to every revision)",
            ),
            Self::Inverted {
                introduced,
                removed,
            } => write!(
                f,
                "inverted applies range: `introduced` {introduced} is not before `removed` {removed}"
            ),
        }
    }
}

impl core::error::Error for AppliesRangeError {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rev(text: &str) -> ProtocolRevision {
        text.parse().unwrap()
    }

    #[test]
    fn half_open_interval_includes_introduced_and_excludes_removed() {
        let range = AppliesRange::new(Some(rev("2025-11-25")), Some(rev("2026-07-28"))).unwrap();
        // Lower bound inclusive, upper bound exclusive — pins `>=` against `>` and `<`
        // against `<=` on the exact boundaries.
        assert!(range.contains(rev("2025-11-25")));
        assert!(!range.contains(rev("2026-07-28")));
        // Strictly inside, and strictly outside each end.
        assert!(range.contains(rev("2026-01-01")));
        assert!(!range.contains(rev("2025-03-26")));
        assert!(!range.contains(rev("2027-01-01")));
    }

    #[test]
    fn open_lower_bound_admits_every_earlier_revision() {
        let range = AppliesRange::new(None, Some(rev("2026-07-28"))).unwrap();
        assert!(range.contains(rev("2020-01-01")));
        assert!(range.contains(rev("2026-07-27")));
        assert!(!range.contains(rev("2026-07-28")));
        assert_eq!(range.introduced(), None);
        assert_eq!(range.removed(), Some(rev("2026-07-28")));
    }

    #[test]
    fn open_upper_bound_admits_every_later_revision() {
        let range = AppliesRange::new(Some(rev("2025-11-25")), None).unwrap();
        assert!(!range.contains(rev("2025-03-26")));
        assert!(range.contains(rev("2025-11-25")));
        assert!(range.contains(rev("2099-12-31")));
        assert_eq!(range.introduced(), Some(rev("2025-11-25")));
        assert_eq!(range.removed(), None);
    }

    #[test]
    fn both_conditions_must_hold_for_membership() {
        // A revision below the lower bound but also below the upper bound is still out:
        // this distinguishes the `&&` from a `||`.
        let range = AppliesRange::new(Some(rev("2025-11-25")), Some(rev("2026-07-28"))).unwrap();
        assert!(!range.contains(rev("2020-01-01"))); // before lo, before hi
        assert!(!range.contains(rev("2030-01-01"))); // after lo, after hi
    }

    #[test]
    fn rejects_empty_range() {
        assert_eq!(AppliesRange::new(None, None), Err(AppliesRangeError::Empty));
    }

    #[test]
    fn rejects_equal_and_inverted_bounds() {
        // Equal bounds describe a zero-width half-open interval (matches nothing); pins
        // that the guard is `>=`, not `>`.
        let same = rev("2026-07-28");
        assert_eq!(
            AppliesRange::new(Some(same), Some(same)),
            Err(AppliesRangeError::Inverted {
                introduced: same,
                removed: same,
            })
        );
        // Backwards bounds.
        assert_eq!(
            AppliesRange::new(Some(rev("2026-07-28")), Some(rev("2025-11-25"))),
            Err(AppliesRangeError::Inverted {
                introduced: rev("2026-07-28"),
                removed: rev("2025-11-25"),
            })
        );
    }

    #[test]
    fn single_bounds_are_accepted() {
        assert!(AppliesRange::new(Some(rev("2025-11-25")), None).is_ok());
        assert!(AppliesRange::new(None, Some(rev("2026-07-28"))).is_ok());
    }

    #[test]
    fn serde_round_trips_each_shape() {
        for (json, introduced, removed) in [
            (
                r#"{"introduced":"2025-11-25","removed":"2026-07-28"}"#,
                Some("2025-11-25"),
                Some("2026-07-28"),
            ),
            (r#"{"introduced":"2025-11-25"}"#, Some("2025-11-25"), None),
            (r#"{"removed":"2026-07-28"}"#, None, Some("2026-07-28")),
        ] {
            let range: AppliesRange = serde_json::from_str(json).unwrap();
            assert_eq!(range.introduced(), introduced.map(rev));
            assert_eq!(range.removed(), removed.map(rev));
            // Absent bounds are omitted from the serialized form, not emitted as null.
            assert_eq!(serde_json::to_string(&range).unwrap(), json);
        }
    }

    #[test]
    fn deserialization_enforces_the_invariants_and_rejects_unknown_fields() {
        assert!(serde_json::from_str::<AppliesRange>("{}").is_err());
        assert!(
            serde_json::from_str::<AppliesRange>(
                r#"{"introduced":"2026-07-28","removed":"2025-11-25"}"#
            )
            .is_err()
        );
        // A misspelled bound is a load error, never a silently open end.
        assert!(serde_json::from_str::<AppliesRange>(r#"{"removes":"2026-07-28"}"#).is_err());
    }

    #[test]
    fn error_messages_carry_actionable_detail() {
        assert!(
            AppliesRangeError::Empty
                .to_string()
                .contains("at least one")
        );
        let inverted = AppliesRangeError::Inverted {
            introduced: rev("2026-07-28"),
            removed: rev("2025-11-25"),
        };
        let message = inverted.to_string();
        assert!(message.contains("2026-07-28"), "{message}");
        assert!(message.contains("2025-11-25"), "{message}");
    }
}
