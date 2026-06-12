// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The bounded loop against the real everything server, in-process over
//! `tokio::io::duplex` — every stop condition (the first M3 definition-of-done line) and the
//! scripted handlers exercised by a server that actually asks.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_everything_server::EverythingServer;
use mcp_reference_host::handler::{HostEvent, HostHandler};
use mcp_reference_host::run::{CallPolicy, PlannedCall, RunPlan, StopReason, run};
use mcp_reference_host::script::InteractionScript;
use rmcp::ServiceExt as _;
use rmcp::service::{RoleClient, RunningService};
use tokio_util::sync::CancellationToken;

async fn connect(
    script: InteractionScript,
) -> (RunningService<RoleClient, HostHandler>, HostHandler) {
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(server) = EverythingServer::new().serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    let handler = HostHandler::new(script);
    let client = handler
        .clone()
        .serve(client_io)
        .await
        .expect("host initializes");
    (client, handler)
}

fn call(tool: &str, arguments: &serde_json::Value) -> PlannedCall {
    PlannedCall {
        tool: tool.to_owned(),
        arguments: arguments.as_object().cloned(),
    }
}

#[tokio::test]
async fn scripted_plan_completes_with_pinned_outcomes() {
    let (client, _) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 10,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![
            call("echo", &serde_json::json!({"message": "hi"})),
            call("add", &serde_json::json!({"a": 2, "b": 3})),
        ]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::Completed);
    assert_eq!(report.turns, 2);
    assert_eq!(report.errors, 0);
    assert_eq!(
        report.outcomes[0].result.as_deref(),
        Ok("Echo: hi"),
        "{report:?}"
    );
    assert_eq!(
        report.outcomes[1].result.as_deref(),
        Ok("The sum of 2 and 3 is 5."),
        "{report:?}"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn turn_limit_stops_the_loop_mid_plan() {
    let (client, _) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 1,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![
            call("echo", &serde_json::json!({"message": "one"})),
            call("echo", &serde_json::json!({"message": "two"})),
            call("echo", &serde_json::json!({"message": "three"})),
        ]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::TurnLimit);
    assert_eq!(report.turns, 1, "exactly one call was spent: {report:?}");
    assert_eq!(report.outcomes.len(), 1);
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn protocol_errors_exhaust_the_budget() {
    let (client, _) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 10,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![
            call("no-such-tool", &serde_json::json!({})),
            call("echo", &serde_json::json!({"message": "never reached"})),
        ]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::ErrorBudgetExhausted);
    assert_eq!(report.turns, 1);
    assert_eq!(report.errors, 1);
    assert!(report.outcomes[0].result.as_deref().is_err(), "{report:?}");
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn in_band_tool_errors_count_against_the_budget_too() {
    // test_error_handling answers isError:true with a 200-shaped result —
    // a budget that ignored in-band failures would loop forever on a
    // persistently failing tool.
    let (client, _) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 10,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![call("test_error_handling", &serde_json::json!({}))]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::ErrorBudgetExhausted);
    assert_eq!(report.errors, 1);
    let detail = report.outcomes[0].result.as_deref().unwrap_err();
    assert!(
        detail.contains("intentionally returns an error"),
        "the outcome carries the tool's own message: {detail}"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn a_budget_tolerates_exactly_its_count() {
    let (client, _) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 10,
        error_budget: 1,
        calls: CallPolicy::Scripted(vec![
            call("no-such-tool", &serde_json::json!({})),
            call("echo", &serde_json::json!({"message": "recovered"})),
        ]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(
        report.stop,
        StopReason::Completed,
        "one error within a budget of one: {report:?}"
    );
    assert_eq!(report.errors, 1);
    assert_eq!(report.outcomes[1].result.as_deref(), Ok("Echo: recovered"));
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn cancellation_wins_before_any_call() {
    let (client, _) = connect(InteractionScript::default()).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let plan = RunPlan {
        turn_limit: 10,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![call("echo", &serde_json::json!({"message": "no"}))]),
    };
    let report = run(&client, &plan, &cancel).await;
    assert_eq!(report.stop, StopReason::Cancelled);
    assert_eq!(report.turns, 0, "no call is spent after cancellation");
    assert!(report.outcomes.is_empty());
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn sep1034_defaults_round_trip_through_a_real_elicitation() {
    // The suite's elicitation-sep1034-client-defaults behavior, end to end
    // against the same tool the server-side suite run exercises: the server
    // elicits a five-type schema with defaults, the scripted handler accepts
    // by filling exactly those defaults, and the tool's reply proves what
    // arrived on the wire (BTreeMap key order).
    let (client, handler) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 1,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![call(
            "test_elicitation_sep1034_defaults",
            &serde_json::json!({}),
        )]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::Completed, "{report:?}");
    assert_eq!(
        report.outcomes[0].result.as_deref(),
        Ok(
            "Elicitation completed: action=accept, content={\"age\":30,\"name\":\"John Doe\",\
             \"score\":95.5,\"status\":\"active\",\"verified\":true}"
        ),
        "{report:?}"
    );
    assert!(
        handler
            .events()
            .contains(&HostEvent::FormElicitationAnswered("accept")),
        "{:?}",
        handler.events()
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn sampling_is_answered_from_the_script() {
    let (client, handler) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 1,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![call(
            "test_sampling",
            &serde_json::json!({"prompt": "Say hello"}),
        )]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::Completed, "{report:?}");
    assert_eq!(
        report.outcomes[0].result.as_deref(),
        Ok("LLM response: Scripted response")
    );
    assert_eq!(handler.events(), vec![HostEvent::SamplingAnswered]);
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn discovery_policy_calls_every_tool_with_synthesized_arguments() {
    // The generic plan the suite's tools_call scenario relies on: list, then
    // call each tool once with schema-derived arguments. Against the
    // everything server some tools fail by design (test_error_handling) or
    // dislike probe arguments — the loop's job is to survive on budget and
    // the known-good tools must succeed.
    let (client, _) = connect(InteractionScript::default()).await;
    let plan = RunPlan {
        turn_limit: 64,
        error_budget: 16,
        calls: CallPolicy::EachDiscoveredToolOnce,
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::Completed, "{report:?}");
    assert!(
        report.turns >= 10,
        "the everything server lists its surface"
    );
    let outcome_of = |tool: &str| {
        report
            .outcomes
            .iter()
            .find(|outcome| outcome.tool == tool)
            .unwrap_or_else(|| panic!("{tool} was discovered and called"))
            .result
            .as_deref()
    };
    assert_eq!(outcome_of("add"), Ok("The sum of 7 and 7 is 14."));
    assert_eq!(outcome_of("echo"), Ok("Echo: probe"));
    assert_eq!(
        outcome_of("get-structured-content"),
        Ok("{\"conditions\":\"Cloudy\",\"humidity\":82.0,\"temperature\":33.0}"),
        "first enum value selects New York"
    );
    client.cancel().await.expect("clean shutdown");
}
