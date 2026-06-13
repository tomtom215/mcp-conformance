// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The compliant SSE-resumption client (feature `http`).
//!
//! `2025-11-25` transports §resumability: after an SSE stream ends without
//! delivering a pending response, the client reconnects with `GET`, waits
//! the server-named `retry` interval first, and offers `Last-Event-ID` so
//! the server can resume the stream. The suite's `sse-retry` scenario
//! measures exactly that dance (−50/+200 ms around `retry`, > 2× fails,
//! `Last-Event-ID` wanted).
//!
//! rmcp 1.7 cannot pass it from its transport surface — measured at source
//! (ADR-0009 §Amendment): POST response streams are wrapped without
//! reconnection logic (`raw_sse_to_jsonrpc`), so the in-flight request is
//! simply lost when its stream closes, and the standalone GET stream — the
//! one wrapper that *does* honor `retry`/`Last-Event-ID` — opens immediately
//! after initialization, which the scenario's clock reads as a too-early
//! reconnect. So the host implements the dance itself, on rmcp's **public**
//! [`StreamableHttpClient`] seam (the official reqwest implementation
//! underneath — no parallel HTTP stack), with the server-named delay
//! honored through [`RetryPolicy::delay_honoring_retry_after`], the policy
//! shipped for exactly this purpose.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use rmcp::model::ClientJsonRpcMessage;
use rmcp::transport::streamable_http_client::{StreamableHttpClient, StreamableHttpPostResponse};

use crate::retry::RetryPolicy;

/// What the dance observed — the binary prints this for the run record.
#[derive(Debug)]
pub struct ResumeReport {
    /// The text content of the tool result delivered after resumption.
    pub tool_result_text: String,
    /// The server-named retry delay that was honored (zero when the result
    /// arrived without a reconnect).
    pub waited: Duration,
    /// The `Last-Event-ID` offered on reconnect.
    pub last_event_id: Option<String>,
}

/// Why the dance failed; every variant names the step so the operator knows
/// where the exchange broke.
#[derive(Debug)]
pub enum ResumeError {
    /// A POST step failed at the transport level.
    Post(&'static str, String),
    /// A step answered with the wrong shape (e.g. JSON where SSE was due).
    UnexpectedShape(&'static str, &'static str),
    /// The SSE stream errored mid-read.
    Stream(String),
    /// The overall deadline elapsed before the result arrived.
    DeadlineElapsed(&'static str),
}

impl std::fmt::Display for ResumeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Post(step, error) => write!(f, "{step}: POST failed: {error}"),
            Self::UnexpectedShape(step, got) => {
                write!(f, "{step}: unexpected response shape ({got})")
            }
            Self::Stream(error) => write!(f, "SSE stream error: {error}"),
            Self::DeadlineElapsed(step) => {
                write!(
                    f,
                    "{step}: no result before the deadline — the server never delivered"
                )
            }
        }
    }
}

impl std::error::Error for ResumeError {}

/// Hard ceiling on each stream read and on the whole dance: the suite kills
/// the client at 30 s, and a clean typed failure beats a kill.
const STEP_DEADLINE: Duration = Duration::from_secs(20);

/// The JSON-RPC id the tool call uses; the resumed result is matched by it.
const CALL_ID: u64 = 3;

/// Runs the resumption dance against `url` with rmcp's reqwest client.
///
/// # Errors
///
/// Returns [`ResumeError`] naming the failed step.
pub async fn run_sse_retry(url: &str) -> Result<ResumeReport, ResumeError> {
    drive(reqwest::Client::default(), url).await
}

/// One POST through the seam, with the step name carried into any error.
async fn post<C: StreamableHttpClient + Sync>(
    client: &C,
    url: &Arc<str>,
    session: Option<Arc<str>>,
    message: ClientJsonRpcMessage,
    step: &'static str,
) -> Result<StreamableHttpPostResponse, ResumeError> {
    client
        .post_message(Arc::clone(url), message, session, None, HashMap::new())
        .await
        .map_err(|error| ResumeError::Post(step, error.to_string()))
}

/// The dance itself, generic over the official client seam so tests drive it
/// with a scripted fake.
async fn drive<C: StreamableHttpClient + Sync>(
    client: C,
    url: &str,
) -> Result<ResumeReport, ResumeError> {
    let url: Arc<str> = url.into();
    let session = handshake(&client, &url).await?;

    // tools/call — the scenario's `test_reconnection` answers over SSE,
    // primes `retry`, then closes the stream without the result.
    let response = post(
        &client,
        &url,
        session.clone(),
        message(serde_json::json!({
            "jsonrpc": "2.0", "id": CALL_ID, "method": "tools/call",
            "params": {"name": "test_reconnection", "arguments": {}},
        })),
        "tools/call",
    )
    .await?;
    let mut call_stream = match response {
        StreamableHttpPostResponse::Sse(stream, _) => stream,
        StreamableHttpPostResponse::Json(value, _) => {
            // A server that answers immediately needs no resumption.
            let value = serde_json::to_value(&value)
                .map_err(|error| ResumeError::Post("tools/call", error.to_string()))?;
            return Ok(ResumeReport {
                tool_result_text: result_text(&value),
                waited: Duration::ZERO,
                last_event_id: None,
            });
        }
        StreamableHttpPostResponse::Accepted => {
            return Err(ResumeError::UnexpectedShape("tools/call", "202 Accepted"));
        }
        _ => {
            return Err(ResumeError::UnexpectedShape(
                "tools/call",
                "unknown variant",
            ));
        }
    };

    // Read the call stream to its end, tracking `retry` and event ids.
    let mut server_retry: Option<Duration> = None;
    let mut last_event_id: Option<String> = None;
    if let Some(result) = read_message_with_id(
        &mut call_stream,
        CALL_ID,
        &mut server_retry,
        &mut last_event_id,
    )
    .await?
    {
        return Ok(ResumeReport {
            tool_result_text: result_text(&result),
            waited: Duration::ZERO,
            last_event_id: None,
        });
    }

    // The stream closed without the result: reconnect and resume.
    resume_pending(&client, &url, session, server_retry, last_event_id).await
}

/// Steps 1–2: `initialize` (returning the server-named session, if any) and
/// `notifications/initialized`.
async fn handshake<C: StreamableHttpClient + Sync>(
    client: &C,
    url: &Arc<str>,
) -> Result<Option<Arc<str>>, ResumeError> {
    let response = post(
        client,
        url,
        None,
        message(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "mcp-reference-host", "version": env!("CARGO_PKG_VERSION")},
            },
        })),
        "initialize",
    )
    .await?;
    let session: Option<Arc<str>> = match response {
        StreamableHttpPostResponse::Json(_, session) => session.map(Into::into),
        StreamableHttpPostResponse::Sse(mut stream, session) => {
            // A spec-shaped server may frame the initialize result as SSE;
            // drain the one result then continue.
            let _ = read_message_with_id(&mut stream, 1, &mut None, &mut None).await?;
            session.map(Into::into)
        }
        StreamableHttpPostResponse::Accepted => {
            return Err(ResumeError::UnexpectedShape("initialize", "202 Accepted"));
        }
        // The enum is #[non_exhaustive]; a future rmcp variant is a shape
        // this dance does not know how to read.
        _ => {
            return Err(ResumeError::UnexpectedShape(
                "initialize",
                "unknown variant",
            ));
        }
    };

    let _ = post(
        client,
        url,
        session.clone(),
        message(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        "notifications/initialized",
    )
    .await?;
    Ok(session)
}

/// Steps 5–6: honor the server-named delay (the policy clamps a hostile
/// value and applies the retry budget), reconnect via GET offering
/// `Last-Event-ID`, and read the pending result off the resumed stream.
async fn resume_pending<C: StreamableHttpClient + Sync>(
    client: &C,
    url: &Arc<str>,
    session: Option<Arc<str>>,
    server_retry: Option<Duration>,
    last_event_id: Option<String>,
) -> Result<ResumeReport, ResumeError> {
    let policy = RetryPolicy::default();
    let waited = server_retry.map_or_else(
        // No `retry` named: the policy's own first-retry backoff applies.
        || policy.delay_for_retry(1, 0.0).unwrap_or(Duration::ZERO),
        |named| {
            policy
                .delay_honoring_retry_after(1, named)
                .unwrap_or(Duration::ZERO)
        },
    );
    tokio::time::sleep(waited).await;

    let session_for_get: Arc<str> = session.unwrap_or_else(|| Arc::from(""));
    let mut get_stream = client
        .get_stream(
            Arc::clone(url),
            session_for_get,
            last_event_id.clone(),
            None,
            HashMap::new(),
        )
        .await
        .map_err(|error| ResumeError::Post("GET reconnect", error.to_string()))?;

    let result = read_message_with_id(&mut get_stream, CALL_ID, &mut None, &mut None)
        .await?
        .ok_or(ResumeError::DeadlineElapsed("resumed GET stream"))?;
    Ok(ResumeReport {
        tool_result_text: result_text(&result),
        waited,
        last_event_id,
    })
}

/// Builds a [`ClientJsonRpcMessage`] from its wire JSON. The wire shapes in
/// this module are fixed strings, so a failure is a programming error caught
/// by every test that exercises the dance.
fn message(value: serde_json::Value) -> ClientJsonRpcMessage {
    #[allow(clippy::expect_used)]
    serde_json::from_value(value).expect("wire-shaped JSON-RPC literal")
}

/// Reads SSE frames until a JSON-RPC message with `id` arrives (returning
/// its JSON), the stream ends (returning `None`), or the step deadline
/// elapses. `retry`/event-id fields are captured into the provided slots as
/// they stream past.
async fn read_message_with_id(
    stream: &mut futures::stream::BoxStream<'static, Result<sse_stream::Sse, sse_stream::Error>>,
    id: u64,
    server_retry: &mut Option<Duration>,
    last_event_id: &mut Option<String>,
) -> Result<Option<serde_json::Value>, ResumeError> {
    let deadline = tokio::time::Instant::now() + STEP_DEADLINE;
    loop {
        let next = tokio::time::timeout_at(deadline, stream.next())
            .await
            .map_err(|_| ResumeError::DeadlineElapsed("SSE read"))?;
        let Some(frame) = next else {
            return Ok(None); // graceful close
        };
        let frame = frame.map_err(|error| ResumeError::Stream(error.to_string()))?;
        if let Some(retry) = frame.retry {
            *server_retry = Some(Duration::from_millis(retry));
        }
        if let Some(event_id) = &frame.id {
            *last_event_id = Some(event_id.clone());
        }
        let Some(data) = frame.data else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) else {
            continue; // priming/keep-alive frames carry empty or non-JSON data
        };
        if value.get("id").and_then(serde_json::Value::as_u64) == Some(id) {
            return Ok(Some(value));
        }
    }
}

/// The first text block of a `tools/call` result, for the run record.
fn result_text(message: &serde_json::Value) -> String {
    message
        .get("result")
        .and_then(|result| result.get("content"))
        .and_then(|content| content.get(0))
        .and_then(|block| block.get("text"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests;
