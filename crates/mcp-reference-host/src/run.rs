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
    ///
    /// `None` allows exactly as many turns as the plan has calls. Every plan is
    /// finite by construction — a script, or one call per tool in a single
    /// listing — so the plan is already the bound; a fixed number under it only
    /// truncates a server that publishes more tools than someone guessed.
    /// `Some` is for a caller that wants a tighter cap than the plan.
    pub turn_limit: Option<u32>,
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
    /// The tool is documented to fail: an in-band `isError: true` result is
    /// the answer it promises, so it is recorded as expected rather than
    /// counted against the error budget.
    ///
    /// The expectation is held both ways: a *success* from such a tool, or a
    /// protocol error instead of the in-band result, still counts — the call
    /// is checked against its documentation, not excused from checking.
    pub fails_by_design: bool,
}

/// Tools a server publishes *in order to* fail, by the name that says so.
///
/// `test_error_handling` is the official suite's contract, not this
/// workspace's invention: the `tools-call-error` server scenario (suite
/// `0.1.16`) requires a tool of exactly that name to answer with an in-band
/// `isError: true` result. Matched by exact name rather than by reading the
/// description, so a third-party tool whose prose merely mentions errors is
/// judged like any other.
pub const FAILS_BY_DESIGN: &[&str] = &["test_error_handling"];

/// Whether `tool` is one of the [`FAILS_BY_DESIGN`] tools.
#[must_use]
pub fn fails_by_design(tool: &str) -> bool {
    FAILS_BY_DESIGN.contains(&tool)
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
    /// The call was [`PlannedCall::fails_by_design`] and answered with the
    /// in-band error result it documents: `result` is `Err`, and it is not
    /// counted in [`RunReport::errors`].
    pub expected_failure: bool,
}

/// The completed run, accounted.
#[derive(Debug, Clone)]
pub struct RunReport {
    /// Tool calls executed (= `outcomes.len()`).
    pub turns: u32,
    /// Calls the plan held, once resolved (the listing's length under
    /// [`CallPolicy::EachDiscoveredToolOnce`]).
    pub planned: u32,
    /// Errors observed (protocol errors and in-band `isError` results), less
    /// the expected failures.
    pub errors: u32,
    /// In-band error results from [`PlannedCall::fails_by_design`] calls —
    /// the answer those tools document, so not counted in `errors`.
    pub expected_failures: u32,
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
        planned: 0,
        errors: 0,
        expected_failures: 0,
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
    report.planned = u32::try_from(calls.len()).unwrap_or(u32::MAX);
    let turn_limit = plan.turn_limit.unwrap_or(report.planned);

    for call in calls {
        if cancel.is_cancelled() {
            report.stop = StopReason::Cancelled;
            return report;
        }
        if report.turns >= turn_limit {
            report.stop = StopReason::TurnLimit;
            return report;
        }

        // The loop owns the plan entry: move the arguments instead of
        // cloning them (clippy 1.88's assigning_clones caught the clone).
        let PlannedCall {
            tool,
            arguments,
            fails_by_design,
        } = call;
        let mut params = CallToolRequestParams::new(tool.clone());
        params.arguments = arguments;
        // Set before the call so rmcp's own `_meta` injection (protocol
        // version, client capabilities) *extends* this map rather than
        // replacing it — both end up on the wire, which is the shape the
        // revision requires.
        params.meta = call_meta(plan);
        let outcome = client.call_tool(params).await;
        report.turns += 1;

        let (result, expected_failure) = judge_outcome(outcome, fails_by_design);
        if expected_failure {
            report.expected_failures += 1;
        } else if result.is_err() {
            report.errors += 1;
        }
        report.outcomes.push(CallOutcome {
            tool,
            result,
            expected_failure,
        });

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

/// Renders one call's outcome, and whether it was the failure the tool
/// documents.
///
/// Every `Err` that is not an expected failure is an error the caller counts:
/// protocol errors, in-band `isError` results from ordinary tools, and a
/// success from a tool [`fails_by_design`] — which broke its documentation as
/// surely as an ordinary tool that failed.
fn judge_outcome(
    outcome: Result<rmcp::model::CallToolResult, rmcp::ServiceError>,
    fails_by_design: bool,
) -> (Result<String, String>, bool) {
    match outcome {
        Ok(result) => {
            let text = result
                .content
                .first()
                .and_then(|content| content.as_text())
                .map(|text| text.text.clone())
                .unwrap_or_default();
            match (result.is_error == Some(true), fails_by_design) {
                (true, expected) => (Err(format!("tool error: {text}")), expected),
                (false, true) => (
                    Err(format!(
                        "documented to fail with an error result, but succeeded: {text}"
                    )),
                    false,
                ),
                (false, false) => (Ok(text), false),
            }
        }
        Err(error) => (Err(error.to_string()), false),
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
                        fails_by_design: fails_by_design(&tool.name),
                    })
                    .collect(),
            ),
            Err(error) => {
                report.errors += 1;
                report.outcomes.push(CallOutcome {
                    tool: "tools/list".to_owned(),
                    result: Err(error.to_string()),
                    expected_failure: false,
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
mod tests;
