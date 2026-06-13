// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The pinned suite's client scenarios, as scripts (ADR-0009: scenario
//! knowledge lives in one table the suite pin governs; unknown scenarios get
//! the generic discover-and-call plan).
//!
//! What each `2025-11-25` protocol scenario requires of the client, decoded
//! from the `0.1.16` bundle (verification record in ADR-0009 §Amendment):
//!
//! - `initialize` — a spec-shaped `initialize` handshake; the scenario's
//!   sessionless server judges only the request, so connecting and exiting
//!   cleanly is the whole plan.
//! - `tools_call` — discover and call `add_numbers` with numeric arguments;
//!   the scenario marks `tool-add-numbers` SUCCESS when the call arrives.
//! - `elicitation-sep1034-client-defaults` — call
//!   `test_client_elicitation_defaults`; the server elicits a five-property
//!   schema and the client must accept with every schema default filled —
//!   [`InteractionScript::default`]'s `AcceptWithDefaults` policy.
//! - `sse-retry` — not an agent-loop scenario at all: the server closes the
//!   `tools/call` SSE stream after a priming event and judges the GET
//!   reconnect's timing (`retry: 500`, −50/+200 ms; > 2× fails), wanting
//!   `Last-Event-ID`. rmcp 1.7's transport cannot pass it (measured —
//!   ADR-0009 §Amendment), so the host runs its own compliant resumption
//!   dance (the `resume` module, feature `http`).

use crate::run::{CallPolicy, RunPlan};
use crate::script::InteractionScript;

/// How the host should behave for one suite scenario.
#[derive(Debug)]
pub enum ScenarioPlan {
    /// Drive the rmcp client through the bounded loop.
    Agent {
        /// Scripted model/user behavior.
        script: InteractionScript,
        /// What to call and what to spend.
        plan: RunPlan,
    },
    /// Run the SSE-resumption dance (the `resume` module, feature `http` —
    /// a doc link would dangle in default-feature builds); the bounded loop
    /// does not apply.
    SseRetry,
}

/// The plan for `scenario` (the `MCP_CONFORMANCE_SCENARIO` value); `None` or
/// an unknown name selects the generic discover-and-call plan.
#[must_use]
pub fn plan_for(scenario: Option<&str>) -> ScenarioPlan {
    match scenario {
        Some("initialize") => ScenarioPlan::Agent {
            script: InteractionScript::default(),
            // The handshake happens on connect; an empty plan then exits
            // cleanly — exactly what the scenario's checks judge.
            plan: RunPlan {
                turn_limit: 0,
                error_budget: 0,
                calls: CallPolicy::Scripted(Vec::new()),
            },
        },
        Some("sse-retry") => ScenarioPlan::SseRetry,
        // tools_call, elicitation-sep1034-client-defaults, and anything this
        // table does not know: list the tools, call each once with
        // schema-derived arguments, answer interactions from the default
        // script. Both named scenarios publish exactly one tool, so "each
        // once" is precisely the call they require.
        _ => ScenarioPlan::Agent {
            script: InteractionScript::default(),
            plan: RunPlan {
                turn_limit: 16,
                error_budget: 0,
                calls: CallPolicy::EachDiscoveredToolOnce,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::ElicitationPolicy;

    #[test]
    fn initialize_is_an_empty_clean_exit_plan() {
        let ScenarioPlan::Agent { plan, .. } = plan_for(Some("initialize")) else {
            panic!("initialize is an agent plan");
        };
        assert!(matches!(plan.calls, CallPolicy::Scripted(ref calls) if calls.is_empty()));
    }

    #[test]
    fn sse_retry_is_the_resumption_dance() {
        assert!(matches!(
            plan_for(Some("sse-retry")),
            ScenarioPlan::SseRetry
        ));
    }

    #[test]
    fn known_tool_scenarios_and_unknowns_discover_and_call() {
        for scenario in [
            Some("tools_call"),
            Some("elicitation-sep1034-client-defaults"),
            Some("anything-future"),
            None,
        ] {
            let ScenarioPlan::Agent { script, plan } = plan_for(scenario) else {
                panic!("{scenario:?} is an agent plan");
            };
            assert!(
                matches!(plan.calls, CallPolicy::EachDiscoveredToolOnce),
                "{scenario:?}"
            );
            assert!(plan.turn_limit >= 1, "{scenario:?} must afford a call");
            // SEP-1034 hinges on this policy: defaults filled, not invented.
            assert_eq!(script.elicitation, ElicitationPolicy::AcceptWithDefaults);
        }
    }
}
