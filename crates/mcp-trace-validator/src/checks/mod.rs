// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks: what one is, and what every one of them promises.
//!
//! A *check* is a pure function from a [`TraceContext`] to findings, registered under a
//! stable ID that requirement-registry entries reference. The contract for every check:
//!
//! - **Falsifiable**: the corpus contains at least one trace it passes and one it fails
//!   (enforced by the corpus invariant test).
//! - **Deterministic**: findings are emitted in event order with stable details.
//! - **Lenient input, precise output**: checks never refuse malformed messages — they
//!   report them.
//! - **Its own clause, and no neighbour's**: the engine attributes a check's finding to
//!   every requirement naming it, so a check that bundles adjacent rules makes each of
//!   them unable to say which one broke. Requirements share a check only where they
//!   state one rule across several sections.
//!
//! The registered list itself lives in the private `inventory` module, re-exported
//! here as [`ALL`].

mod base;
#[cfg(feature = "draft-2026-07-28")]
mod draft;
mod inventory;
mod lifecycle;
mod negotiation;
mod prompts;
mod resources;
mod support;
mod tools;
mod transport;
mod utilities;

use crate::context::TraceContext;
use crate::report::Finding;

/// A check function: examines the trace, pushes findings into the sink.
type CheckFn = fn(&TraceContext<'_>, &mut FindingSink);

/// A registered check.
#[derive(Debug, Clone, Copy)]
pub struct Check {
    /// Stable check identifier referenced by registry entries (e.g.
    /// `lifecycle.first-interaction-initialize`).
    pub id: &'static str,
    run: CheckFn,
}

/// What running one check produced.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct CheckOutcome {
    /// Findings, each stamped with the check's ID.
    pub findings: Vec<Finding>,
    /// How many subjects the check examined, when it reports that.
    ///
    /// `None` means the check has not been instrumented, and the engine then
    /// treats it as having examined something — the fail-safe direction. A
    /// wrong `Some(0)` would report a clause as unobserved when it was really
    /// judged and clean, which loses information; a wrong "observed" only
    /// restores today's behaviour.
    pub subjects: Option<u32>,
}

impl Check {
    /// Runs the check, returning its findings and what it examined.
    #[must_use]
    pub fn run(&self, context: &TraceContext<'_>) -> CheckOutcome {
        let mut sink = FindingSink {
            check: self.id,
            findings: Vec::new(),
            subjects: None,
        };
        (self.run)(context, &mut sink);
        CheckOutcome {
            findings: sink.findings,
            subjects: sink.subjects,
        }
    }
}

/// Collects findings on behalf of one check, stamping each with the check ID.
#[derive(Debug)]
pub struct FindingSink {
    check: &'static str,
    findings: Vec<Finding>,
    subjects: Option<u32>,
}

impl FindingSink {
    /// Records a finding at an event (`seq`) with an actionable detail sentence.
    pub fn push(&mut self, seq: Option<u64>, detail: String) {
        self.findings.push(Finding {
            check: self.check.to_owned(),
            seq,
            detail,
        });
    }

    /// Records that this check considered one more subject.
    ///
    /// **The counting rule**, applied identically by every check:
    ///
    /// > A subject is a trace element the check *considered* — one that, with
    /// > different content, could have produced a finding. Count it after the
    /// > filters that define the clause's scope, and before the condition that
    /// > makes an element a violation.
    ///
    /// So a prohibition over server messages counts every server message,
    /// because any of them could have been the violation; a clause about
    /// subscription streams counts only messages on such a stream, because a
    /// session without one gave the clause nothing to bind to. The difference
    /// is what separates "complied with" from "never came up", and reporting
    /// the second as the first states evidence the trace does not carry.
    pub fn examined(&mut self) {
        self.subjects = Some(self.subjects.unwrap_or(0).saturating_add(1));
    }

    /// Records that this check considered `count` subjects at once.
    ///
    /// The same rule as [`Self::examined`], for checks that compute their
    /// subject set before looping — or that legitimately examined none.
    pub fn examined_many(&mut self, count: usize) {
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        self.subjects = Some(self.subjects.unwrap_or(0).saturating_add(count));
    }
}

pub use inventory::{ALL, find};
