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

impl Check {
    /// Runs the check, returning its findings tagged with this check's ID.
    #[must_use]
    pub fn run(&self, context: &TraceContext<'_>) -> Vec<Finding> {
        let mut sink = FindingSink {
            check: self.id,
            findings: Vec::new(),
        };
        (self.run)(context, &mut sink);
        sink.findings
    }
}

/// Collects findings on behalf of one check, stamping each with the check ID.
#[derive(Debug)]
pub struct FindingSink {
    check: &'static str,
    findings: Vec<Finding>,
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
}

pub use inventory::{ALL, find};
