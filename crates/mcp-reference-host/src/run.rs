// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The bounded tool-use loop.
//!
//! A deterministic call policy executes under an explicit stop-condition
//! lattice — cancellation, turn limit, error budget, completion — checked in
//! that order, so every run ends for a reason the report names
//! (02-architecture.md: no "the loop usually terminates").

// SEP-2577 forward-deprecates Logging, and rmcp 3.x carries the attribute, so
// naming `LoggingLevel` fires it on correct code: the level a request asks for
// is how `2026-07-28` *replaced* `logging/setLevel`, and it is the only way a
// recording can carry the logging clauses at all. Scoped to this module rather
// than the crate, matching `mcp-everything-server`'s two module-level allows —
// a blanket allow would also hide a deprecation that genuinely matters.
#![allow(deprecated)]
use rmcp::model::{CallToolRequestParams, LoggingLevel, RequestMetaObject};
use rmcp::service::{Peer, RoleClient, RunningService, Service};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

/// What the loop is allowed to spend and what it should call.
#[derive(Debug, Clone)]
pub struct RunPlan {
    /// Maximum tool calls before the loop stops with [`StopReason::TurnLimit`].
    pub turn_limit: u32,
    /// Errors tolerated before [`StopReason::ErrorBudgetExhausted`]: the run
    /// stops once `errors > error_budget` (a budget of 0 stops on the first).
    pub error_budget: u32,
    /// Which calls to make.
    pub calls: CallPolicy,
    /// The minimum log level to ask each call for, in
    /// `_meta.io.modelcontextprotocol/logLevel`.
    ///
    /// `None` asks for nothing, which is what the suite's scenarios want and
    /// what `2026-07-28` treats as "emit no `notifications/message` for this
    /// request" (LOG-008). A recording sets it, because a server's logging
    /// clauses are unjudgeable against a session that never asked to see a log
    /// line — and asking is the client-side half of the mechanism that
    /// replaced `logging/setLevel`.
    pub log_level: Option<LoggingLevel>,
    /// The W3C Trace Context `traceparent` to carry in each call's `_meta`.
    ///
    /// Supplied rather than generated, because a client does not invent a trace
    /// context — it propagates the one its caller handed it, and a host that
    /// minted a fresh id per run would make every recording of the same session
    /// differ. `None` sends none, which is what the suite's scenarios want.
    pub trace_parent: Option<String>,
}

/// Deterministic call selection.
#[derive(Debug, Clone)]
pub enum CallPolicy {
    /// Exactly these calls, in order.
    Scripted(Vec<PlannedCall>),
    /// `tools/list`, then each discovered tool once in listing order, with
    /// arguments synthesized from its input schema.
    EachDiscoveredToolOnce,
}

/// One scripted tool call.
#[derive(Debug, Clone)]
pub struct PlannedCall {
    /// Tool name.
    pub tool: String,
    /// Arguments object (`None` for tools taking none).
    pub arguments: Option<Map<String, Value>>,
}

/// Why the loop stopped. Exactly one reason per run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The plan ran to its end.
    Completed,
    /// The turn limit was reached with calls still planned.
    TurnLimit,
    /// More errors occurred than the budget tolerates.
    ErrorBudgetExhausted,
    /// The cancellation token fired.
    Cancelled,
}

/// One executed call, as observed.
#[derive(Debug, Clone)]
pub struct CallOutcome {
    /// Tool name as called.
    pub tool: String,
    /// `Ok` carries the first text block (empty string when none); `Err`
    /// carries the protocol error or in-band tool error, rendered.
    pub result: Result<String, String>,
}

/// The completed run, accounted.
#[derive(Debug, Clone)]
pub struct RunReport {
    /// Tool calls executed (= `outcomes.len()`).
    pub turns: u32,
    /// Errors observed (protocol errors and in-band `isError` results).
    pub errors: u32,
    /// Why the loop ended.
    pub stop: StopReason,
    /// Per-call observations, in execution order.
    pub outcomes: Vec<CallOutcome>,
}

/// Runs `plan` against the connected server behind `client` until a stop
/// condition fires.
///
/// Takes the running service rather than its [`Peer`],
/// and the difference is protocol-visible: `RunningService::call_tool` drives
/// SEP-2322's MRTR rounds — on an `input_required` result it fulfils the
/// server's `inputRequests` through this host's own handler and retries,
/// echoing the `requestState` — while `Peer::call_tool` answers a single round
/// and reports anything else as an unexpected response. At `2026-07-28` that
/// is the *only* way a server can ask for sampling or elicitation, so a host
/// on the peer method cannot complete an interactive call at all.
///
/// Listing failures (under [`CallPolicy::EachDiscoveredToolOnce`]) count
/// against the error budget like any other error.
pub async fn run<S: Service<RoleClient>>(
    client: &RunningService<RoleClient, S>,
    plan: &RunPlan,
    cancel: &CancellationToken,
) -> RunReport {
    let peer: &Peer<RoleClient> = client.peer();
    let mut report = RunReport {
        turns: 0,
        errors: 0,
        stop: StopReason::Completed,
        outcomes: Vec::new(),
    };

    let Some(calls) = resolve_calls(peer, &plan.calls, &mut report).await else {
        // Listing failed: the error is recorded; the budget decides.
        if report.errors > plan.error_budget {
            report.stop = StopReason::ErrorBudgetExhausted;
        }
        return report;
    };

    for call in calls {
        if cancel.is_cancelled() {
            report.stop = StopReason::Cancelled;
            return report;
        }
        if report.turns >= plan.turn_limit {
            report.stop = StopReason::TurnLimit;
            return report;
        }

        // The loop owns the plan entry: move the arguments instead of
        // cloning them (clippy 1.88's assigning_clones caught the clone).
        let PlannedCall { tool, arguments } = call;
        let mut params = CallToolRequestParams::new(tool.clone());
        params.arguments = arguments;
        // Set before the call so rmcp's own `_meta` injection (protocol
        // version, client capabilities) *extends* this map rather than
        // replacing it — both end up on the wire, which is the shape the
        // revision requires.
        params.meta = call_meta(plan);
        let outcome = client.call_tool(params).await;
        report.turns += 1;

        let result = judge_outcome(outcome, &mut report.errors);
        report.outcomes.push(CallOutcome { tool, result });

        if report.errors > plan.error_budget {
            report.stop = StopReason::ErrorBudgetExhausted;
            return report;
        }
    }

    report.stop = StopReason::Completed;
    report
}

/// The `_meta` a call carries, or `None` when the plan asks for neither field.
///
/// A function rather than an `if` in the loop, because the condition is an
/// `or` and the loop cannot separate its arms: the capture sets both fields, so
/// an `and` behaves identically there and nothing notices. Pulled out, each
/// arm is one assertion.
fn call_meta(plan: &RunPlan) -> Option<RequestMetaObject> {
    if plan.log_level.is_none() && plan.trace_parent.is_none() {
        return None;
    }
    let mut meta = RequestMetaObject::new();
    if let Some(level) = plan.log_level {
        meta.set_log_level(level);
    }
    if let Some(trace_parent) = plan.trace_parent.as_deref() {
        meta.set_traceparent(trace_parent);
    }
    Some(meta)
}

/// Renders one call's outcome, counting protocol errors and in-band
/// `isError` results against `errors`.
fn judge_outcome(
    outcome: Result<rmcp::model::CallToolResult, rmcp::ServiceError>,
    errors: &mut u32,
) -> Result<String, String> {
    match outcome {
        Ok(result) => {
            let text = result
                .content
                .first()
                .and_then(|content| content.as_text())
                .map(|text| text.text.clone())
                .unwrap_or_default();
            if result.is_error == Some(true) {
                *errors += 1;
                Err(format!("tool error: {text}"))
            } else {
                Ok(text)
            }
        }
        Err(error) => {
            *errors += 1;
            Err(error.to_string())
        }
    }
}

/// Materializes the call list; `None` when discovery itself failed (the
/// failure is already recorded on the report).
async fn resolve_calls(
    peer: &Peer<RoleClient>,
    policy: &CallPolicy,
    report: &mut RunReport,
) -> Option<Vec<PlannedCall>> {
    match policy {
        CallPolicy::Scripted(calls) => Some(calls.clone()),
        CallPolicy::EachDiscoveredToolOnce => match peer.list_tools(None).await {
            Ok(listing) => Some(
                listing
                    .tools
                    .iter()
                    .map(|tool| PlannedCall {
                        tool: tool.name.to_string(),
                        arguments: Some(synthesize_arguments(&tool.input_schema)),
                    })
                    .collect(),
            ),
            Err(error) => {
                report.errors += 1;
                report.outcomes.push(CallOutcome {
                    tool: "tools/list".to_owned(),
                    result: Err(error.to_string()),
                });
                None
            }
        },
    }
}

/// Deterministic sample arguments for a tool's JSON-Schema `inputSchema`.
///
/// Every *required* property gets a fixed value by declared type, with local
/// `$ref`s resolved (schemars derives enums as `$ref` into `$defs`) and enum
/// shapes sampled at their first value. Optional properties are omitted —
/// the smallest conformant call.
#[must_use]
pub fn synthesize_arguments(input_schema: &Map<String, Value>) -> Map<String, Value> {
    let mut arguments = Map::new();
    let Some(required) = input_schema.get("required").and_then(Value::as_array) else {
        return arguments;
    };
    let properties = input_schema.get("properties").and_then(Value::as_object);
    for name in required.iter().filter_map(Value::as_str) {
        let property = properties.and_then(|props| props.get(name));
        arguments.insert(name.to_owned(), sample_value(input_schema, property));
    }
    arguments
}

/// A fixed value satisfying one property schema (refs resolved against
/// `root`, the full `inputSchema` document).
fn sample_value(root: &Map<String, Value>, property: Option<&Value>) -> Value {
    let Some(property) = property.map(|p| resolve_local_refs(root, p)) else {
        return Value::String("probe".to_owned());
    };
    // Enum shapes first: classic `enum`, then schemars' `oneOf` of `const`s.
    if let Some(first) = property
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return first.clone();
    }
    if let Some(first_const) = property
        .get("oneOf")
        .and_then(Value::as_array)
        .and_then(|variants| variants.first())
        .and_then(|variant| variant.get("const"))
    {
        return first_const.clone();
    }
    let type_ = property
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string");
    match type_ {
        "number" | "integer" => Value::from(7),
        "boolean" => Value::Bool(true),
        "array" => Value::Array(Vec::new()),
        "object" => Value::Object(Map::new()),
        _ => Value::String("probe".to_owned()),
    }
}

/// Follows local `$ref`s (`#/$defs/...`, `#/definitions/...`) within `root`,
/// bounded to a small depth so a cyclic schema cannot loop the host.
fn resolve_local_refs<'a>(root: &'a Map<String, Value>, mut schema: &'a Value) -> &'a Value {
    for _ in 0..4 {
        let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
            return schema;
        };
        let Some(path) = reference.strip_prefix("#/") else {
            return schema;
        };
        let mut target: Option<&Value> = None;
        let mut cursor: &Map<String, Value> = root;
        for segment in path.split('/') {
            match cursor.get(segment) {
                Some(value) => {
                    target = Some(value);
                    match value.as_object() {
                        Some(object) => cursor = object,
                        None => break,
                    }
                }
                None => return schema,
            }
        }
        match target {
            Some(resolved) => schema = resolved,
            None => return schema,
        }
    }
    schema
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn plan(log_level: Option<LoggingLevel>, trace_parent: Option<&str>) -> RunPlan {
        RunPlan {
            turn_limit: 1,
            error_budget: 0,
            calls: CallPolicy::EachDiscoveredToolOnce,
            log_level,
            trace_parent: trace_parent.map(str::to_owned),
        }
    }

    /// Each field alone must still produce a `_meta`, which is the arm the
    /// capture cannot exercise: it sets both, so an `and` here would behave
    /// identically on every recording and on every test that drives one.
    #[test]
    fn either_field_alone_is_enough_to_carry_a_meta() {
        assert!(call_meta(&plan(None, None)).is_none(), "neither asked for");

        let logging = call_meta(&plan(Some(LoggingLevel::Debug), None)).unwrap();
        assert!(logging.get_traceparent().is_none(), "{logging:?}");
        assert!(
            serde_json::to_string(&logging)
                .unwrap()
                .contains("logLevel"),
            "{logging:?}"
        );

        let traced = call_meta(&plan(
            None,
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        ))
        .unwrap();
        assert_eq!(
            traced.get_traceparent(),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
        );
        assert!(
            !serde_json::to_string(&traced).unwrap().contains("logLevel"),
            "{traced:?}"
        );

        let both = call_meta(&plan(
            Some(LoggingLevel::Debug),
            Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        ))
        .unwrap();
        assert!(both.get_traceparent().is_some(), "{both:?}");
        assert!(
            serde_json::to_string(&both).unwrap().contains("logLevel"),
            "{both:?}"
        );
    }

    #[test]
    fn synthesized_arguments_cover_required_properties_only() {
        let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" },
                "note": { "type": "string" }
            },
            "required": ["a", "b"]
        }))
        .unwrap();
        let arguments = synthesize_arguments(&schema);
        assert_eq!(arguments.len(), 2, "{arguments:?}");
        assert_eq!(arguments["a"], 7);
        assert_eq!(arguments["b"], 7);
    }

    #[test]
    fn synthesized_arguments_respect_types_and_enums() {
        let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "enum": ["New York", "Chicago"] },
                "flag": { "type": "boolean" },
                "items": { "type": "array" },
                "config": { "type": "object" },
                "message": { "type": "string" }
            },
            "required": ["city", "flag", "items", "config", "message"]
        }))
        .unwrap();
        let arguments = synthesize_arguments(&schema);
        assert_eq!(arguments["city"], "New York", "first enum value wins");
        assert_eq!(arguments["flag"], true);
        assert_eq!(arguments["items"], serde_json::json!([]));
        assert_eq!(
            arguments["config"],
            serde_json::json!({}),
            "object-typed requirements get an empty object, not a string"
        );
        assert_eq!(arguments["message"], "probe");
    }

    #[test]
    fn schemars_ref_enums_resolve_to_their_first_const() {
        // The exact shape `#[derive(JsonSchema)]` emits for a Rust enum:
        // the property is a `$ref` into `$defs`, and the definition is a
        // `oneOf` of `const` variants (get-structured-content's Location).
        let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "$defs": {
                "Location": {
                    "oneOf": [
                        { "const": "New York", "type": "string" },
                        { "const": "Chicago", "type": "string" }
                    ]
                }
            },
            "type": "object",
            "properties": { "location": { "$ref": "#/$defs/Location" } },
            "required": ["location"]
        }))
        .unwrap();
        assert_eq!(synthesize_arguments(&schema)["location"], "New York");
    }

    #[test]
    fn unresolvable_and_cyclic_refs_degrade_to_the_string_probe() {
        // A dangling ref and a two-node cycle: the resolver must stay
        // bounded and total, never loop or panic.
        let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "$defs": {
                "A": { "$ref": "#/$defs/B" },
                "B": { "$ref": "#/$defs/A" }
            },
            "type": "object",
            "properties": {
                "dangling": { "$ref": "#/$defs/Missing" },
                "cyclic": { "$ref": "#/$defs/A" }
            },
            "required": ["dangling", "cyclic"]
        }))
        .unwrap();
        let arguments = synthesize_arguments(&schema);
        assert_eq!(arguments["dangling"], "probe");
        assert_eq!(arguments["cyclic"], "probe");
    }

    #[test]
    fn no_required_block_synthesizes_the_empty_call() {
        let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": { "opt": { "type": "string" } }
        }))
        .unwrap();
        assert!(synthesize_arguments(&schema).is_empty());
    }
}
