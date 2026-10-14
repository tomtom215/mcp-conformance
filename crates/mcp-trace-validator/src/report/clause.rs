// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The clause a finding breaks, as a report row carries it.

use mcp_conformance_core::requirement::{Requirement, SourceRef};
use mcp_conformance_core::revision::ProtocolRevision;
use serde::{Deserialize, Serialize};

/// The clause a failing or warning row was judged against, as the registry records it:
/// enough to act on a finding without opening the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClauseSource {
    /// The spec page and anchor (`basic/lifecycle#initialization`).
    pub section: String,
    /// The clause, verbatim from the spec.
    pub quote: String,
    /// The published page for [`Self::section`]: at the judged revision, or
    /// where an extension publishes its own clauses.
    pub url: String,
}

impl ClauseSource {
    /// The source of `requirement` as published at `revision` — or, for an
    /// extension's clause, where the extension publishes it
    /// ([`Requirement::url`]).
    #[must_use]
    pub fn new(requirement: &Requirement, revision: ProtocolRevision) -> Self {
        Self {
            section: requirement.source.section.clone(),
            quote: requirement.source.quote.clone(),
            url: requirement.url(revision),
        }
    }

    /// `source` as published at `revision`.
    #[must_use]
    pub fn from_source(source: &SourceRef, revision: ProtocolRevision) -> Self {
        Self {
            section: source.section.clone(),
            quote: source.quote.clone(),
            url: source.url(revision),
        }
    }
}
