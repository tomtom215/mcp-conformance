// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Aligning each revision's rows into one row per clause.

use std::collections::HashMap;

use mcp_conformance_core::requirement::RegistrySet;

use super::MultiRow;
use crate::report::{Report, RequirementReport};

/// The union of `reports`' rows, in registry-union order: the set's
/// requirements walked once, keeping each clause that exists at one or more
/// judged revisions.
///
/// A projected report contains exactly the clauses in force at its revision,
/// so a clause's outcome there is "found in that report" and its absence is
/// "not found" — applicability needs no second source of truth.
pub(super) fn align(set: &RegistrySet, reports: &[Report]) -> Vec<MultiRow> {
    let indexes: Vec<HashMap<&str, &RequirementReport>> = reports
        .iter()
        .map(|report| {
            report
                .requirements
                .iter()
                .map(|row| (row.id.as_str(), row))
                .collect()
        })
        .collect();
    let mut rows = Vec::new();
    for requirement in set.requirements() {
        let id = requirement.id.as_str();
        let found: Vec<Option<&RequirementReport>> =
            indexes.iter().map(|index| index.get(id).copied()).collect();
        if found.iter().all(Option::is_none) {
            continue;
        }
        rows.push(MultiRow {
            id: id.to_owned(),
            level: requirement.level.keyword().to_owned(),
            actor: Some(requirement.actor),
            outcomes: found.iter().map(|row| row.map(|row| row.outcome)).collect(),
            findings: found
                .iter()
                .map(|row| row.map(|row| row.findings.clone()).unwrap_or_default())
                .collect(),
            sources: found
                .iter()
                .map(|row| row.and_then(|row| row.source.clone()))
                .collect(),
        });
    }
    rows
}
