// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The check inventory.
//!
//! A *check* is a pure function from a [`TraceContext`] to findings, registered under a
//! stable ID that requirement-registry entries reference. The contract for every check:
//!
//! - **Falsifiable**: the corpus contains at least one trace it passes and one it fails
//!   (enforced by the corpus invariant test).
//! - **Deterministic**: findings are emitted in event order with stable details.
//! - **Lenient input, precise output**: checks never refuse malformed messages — they
//!   report them.

mod base;
mod lifecycle;

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

/// Every check implemented by this build, in stable order.
pub static ALL: &[Check] = &[
    Check {
        id: "base.request-id-type",
        run: base::request_id_type,
    },
    Check {
        id: "base.request-id-not-null",
        run: base::request_id_not_null,
    },
    Check {
        id: "base.request-id-unique",
        run: base::request_id_unique,
    },
    Check {
        id: "base.result-id-matches",
        run: base::result_id_matches,
    },
    Check {
        id: "base.notification-no-id",
        run: base::notification_no_id,
    },
    Check {
        id: "base.error-shape",
        run: base::error_shape,
    },
    Check {
        id: "base.error-code-integer",
        run: base::error_code_integer,
    },
    Check {
        id: "base.jsonrpc-version",
        run: base::jsonrpc_version,
    },
    Check {
        id: "base.error-id-matches",
        run: base::error_id_matches,
    },
    Check {
        id: "lifecycle.first-interaction-initialize",
        run: lifecycle::first_interaction_initialize,
    },
    Check {
        id: "lifecycle.initialize-params",
        run: lifecycle::initialize_params,
    },
    Check {
        id: "lifecycle.initialized-notification",
        run: lifecycle::initialized_notification,
    },
    Check {
        id: "lifecycle.client-requests-before-init-response",
        run: lifecycle::client_requests_before_init_response,
    },
    Check {
        id: "lifecycle.server-requests-before-initialized",
        run: lifecycle::server_requests_before_initialized,
    },
    Check {
        id: "lifecycle.initialize-result-version",
        run: lifecycle::initialize_result_version,
    },
];

/// Looks up a check by its stable ID.
#[must_use]
pub fn find(id: &str) -> Option<&'static Check> {
    ALL.iter().find(|check| check.id == id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mcp_conformance_core::requirement::{Registry, Verification};
    use std::collections::HashSet;

    #[test]
    fn check_ids_are_unique() {
        let mut seen = HashSet::new();
        for check in ALL {
            assert!(seen.insert(check.id), "duplicate check id {}", check.id);
        }
    }

    #[test]
    fn builtin_registry_and_check_inventory_cover_each_other_exactly() {
        // Every check the registry references exists, and every implemented check is
        // referenced — drift in either direction is a defect, not a warning.
        let registry = Registry::builtin_2025_11_25().unwrap();
        let mut referenced = HashSet::new();
        for requirement in registry.requirements() {
            if let Verification::Checks { checks } = &requirement.verification {
                for check in checks {
                    assert!(
                        find(check).is_some(),
                        "{}: references unimplemented check {check}",
                        requirement.id
                    );
                    referenced.insert(check.clone());
                }
            }
        }
        for check in ALL {
            assert!(
                referenced.contains(check.id),
                "check {} is implemented but referenced by no requirement",
                check.id
            );
        }
    }
}
