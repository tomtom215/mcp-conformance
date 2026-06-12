// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The host's real transports against the real everything-server app, plus
//! trace capture through them: the streamable HTTP test serves the server's
//! actual axum router on a loopback socket and connects through the
//! reqwest-backed rmcp transport (feature `http`); the captured trace then
//! round-trips the validator's reader and engine — the M3 capture
//! definition-of-done line, on every platform, with no npx and no network
//! beyond loopback.
//!
//! The child-process leg's command construction is unit-tested in
//! `connect`; the spawn itself is proven against the real
//! `mcp-everything-server` binary by `cargo xtask conformance` (orchestration
//! may spawn sibling binaries; `cargo test` cannot reference another crate's
//! `CARGO_BIN_EXE`).

#![cfg(feature = "http")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_everything_server::http::router;
use mcp_everything_server::policy::HttpSecurityPolicy;
use mcp_reference_host::capture::{CaptureTransport, RecordingTransport};
use mcp_reference_host::handler::HostHandler;
use mcp_reference_host::run::{CallPolicy, PlannedCall, RunPlan, StopReason, run};
use mcp_reference_host::script::InteractionScript;
use rmcp::ServiceExt as _;
use tokio_util::sync::CancellationToken;

/// Serves the everything-server app on an OS-assigned loopback port,
/// returning its MCP endpoint URL.
async fn serve_everything() -> String {
    let app = router(HttpSecurityPolicy::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/mcp")
}

#[tokio::test]
async fn http_transport_completes_a_scripted_loop() {
    let url = serve_everything().await;
    let transport = mcp_reference_host::connect::streamable_http(&url);
    let handler = HostHandler::new(InteractionScript::default());
    let client = handler.clone().serve(transport).await.expect("initializes");

    let plan = RunPlan {
        turn_limit: 4,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![
            PlannedCall {
                tool: "echo".to_owned(),
                arguments: serde_json::json!({"message": "over http"})
                    .as_object()
                    .cloned(),
            },
            PlannedCall {
                tool: "add".to_owned(),
                arguments: serde_json::json!({"a": 19, "b": 23}).as_object().cloned(),
            },
        ]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::Completed, "{report:?}");
    assert_eq!(report.outcomes[0].result.as_deref(), Ok("Echo: over http"));
    assert_eq!(
        report.outcomes[1].result.as_deref(),
        Ok("The sum of 19 and 23 is 42.")
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn captured_http_session_validates_through_the_real_engine() {
    let url = serve_everything().await;
    let dir = std::env::temp_dir().join(format!("host-http-capture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.jsonl");

    let transport = RecordingTransport::create(
        mcp_reference_host::connect::streamable_http(&url),
        CaptureTransport::StreamableHttp,
        &path,
    )
    .expect("trace file");
    let handler = HostHandler::new(InteractionScript::default());
    let client = handler.clone().serve(transport).await.expect("initializes");
    let plan = RunPlan {
        turn_limit: 2,
        error_budget: 0,
        calls: CallPolicy::Scripted(vec![PlannedCall {
            tool: "test_elicitation_sep1034_defaults".to_owned(),
            arguments: None,
        }]),
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    assert_eq!(report.stop, StopReason::Completed, "{report:?}");
    client.cancel().await.expect("clean shutdown");

    // The captured bytes parse through the validator's reader and validate
    // with zero MUST-level failures — the host capture's contract. The
    // elicitation round-trip above puts a server-initiated request in the
    // trace, so the capture proves both directions of dispatch, not just
    // request/response.
    let bytes = std::fs::read_to_string(&path).unwrap();
    let events = mcp_trace_validator::reader::parse_trace(
        &bytes,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .expect("captured trace parses");
    assert!(
        events.len() >= 6,
        "handshake + tool call + elicitation round-trip: {}",
        events.len()
    );
    let elicits = bytes.matches("elicitation/create").count();
    assert!(elicits >= 1, "the server-initiated request was captured");

    let registry = mcp_conformance_core::requirement::Registry::builtin_2025_11_25().unwrap();
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    let failures: Vec<&str> = report
        .requirements
        .iter()
        .filter(|row| row.outcome == mcp_trace_validator::report::Outcome::Fail)
        .map(|row| row.id.as_str())
        .collect();
    assert!(
        failures.is_empty(),
        "a faithfully captured clean session must not fail any MUST: {failures:?}\n{}",
        report.render_human()
    );

    let _ = std::fs::remove_dir_all(dir);
}
