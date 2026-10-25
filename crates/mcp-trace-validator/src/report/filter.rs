// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A display filter: which clauses the human report lists.
//!
//! It changes what is *shown* and nothing a report decides (ADR-0025). The rows
//! stay in the report, the totals count every clause, the verdict counts every
//! finding, and the report says how many findings the filter leaves out of the
//! listing — so a filtered report cannot be read as a smaller trace, and a
//! filter can never turn a failing run into a passing one.

use core::fmt::Write as _;

/// The area of a requirement ID: its prefix, as `LIFE` in `LIFE-001`.
#[must_use]
pub fn area_of(id: &str) -> &str {
    id.split_once('-').map_or("", |(area, _)| area)
}

/// Which clauses a human report lists.
///
/// Those in one of `areas` (the ID's prefix, upper case) and at one of `levels`
/// (the keyword a row carries, `MUST NOT`). An empty list selects everything,
/// so the default filter lists every clause.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Filter {
    /// The areas listed; empty for all.
    pub areas: Vec<String>,
    /// The levels listed; empty for all.
    pub levels: Vec<String>,
}

impl Filter {
    /// A filter listing `areas` and `levels` (either may be empty for all).
    #[must_use]
    pub const fn new(areas: Vec<String>, levels: Vec<String>) -> Self {
        Self { areas, levels }
    }

    /// Whether this filter lists every clause.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.areas.is_empty() && self.levels.is_empty()
    }

    /// Whether the clause `id` at `level` is listed.
    ///
    /// ```
    /// use mcp_trace_validator::report::Filter;
    ///
    /// let filter = Filter::new(vec!["BASE".into(), "TOOL".into()], vec!["MUST".into()]);
    /// assert!(filter.matches("TOOL-006", "MUST"));
    /// assert!(!filter.matches("TOOL-006", "SHOULD"));
    /// assert!(!filter.matches("LIFE-001", "MUST"));
    /// assert!(Filter::default().matches("LIFE-001", "MAY"));
    /// ```
    #[must_use]
    pub fn matches(&self, id: &str, level: &str) -> bool {
        (self.areas.is_empty() || self.areas.iter().any(|area| area == area_of(id)))
            && (self.levels.is_empty() || self.levels.iter().any(|wanted| wanted == level))
    }

    /// The filter as a phrase: `BASE, TOOL clauses at MUST, SHOULD`.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut text = if self.areas.is_empty() {
            "all".to_owned()
        } else {
            self.areas.join(", ")
        };
        text.push_str(" clauses");
        if !self.levels.is_empty() {
            let _ = write!(text, " at {}", self.levels.join(", "));
        }
        text
    }

    /// The note a filtered human report carries, with `hidden` findings in the
    /// clauses it leaves out.
    pub(crate) fn note(&self, hidden: usize) -> String {
        let left_out = match hidden {
            0 => "no finding is in the other clauses".to_owned(),
            1 => "1 finding in another clause is not shown".to_owned(),
            many => format!("{many} findings in other clauses are not shown"),
        };
        format!(
            "  NOTE  listing only {} (a display filter): {left_out}.\n        The totals and \
             the verdict count every clause and every finding.\n",
            self.describe()
        )
    }
}
