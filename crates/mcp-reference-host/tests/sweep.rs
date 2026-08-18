// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The feature sweep against the real everything server, in-process over
//! `tokio::io::duplex`.
//!
//! `sweep`'s unit tests pin the pure parts — template substitution, argument
//! synthesis, the report's accounting — and none of them can answer the
//! question this file exists for: *does the sweep actually drive the surface
//! it claims to?* Every step of it is an `await` against a live peer, so the
//! only way to know a step happens at all is to run it against a server and
//! read what came back.
//!
//! Driven at `2025-11-25` rather than the stateless revision, and the reason
//! is that the sweep is revision-agnostic by construction: it asks
//! `prompts/list` what prompts there are and reads the answer. Pinning it to
//! the older surface here proves that, and keeps this test independent of the
//! `draft-2026-07-28` feature gate.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_everything_server::EverythingServer;
use mcp_reference_host::handler::HostHandler;
use mcp_reference_host::script::InteractionScript;
use mcp_reference_host::sweep::{self, SweepReport};
use rmcp::ServiceExt as _;

/// A connected host, with the everything server on the other end of a duplex.
async fn swept() -> SweepReport {
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(server) = EverythingServer::new().serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    let client = HostHandler::new(InteractionScript::default())
        .serve(client_io)
        .await
        .expect("host initializes");
    let report = sweep::run(client.peer()).await;
    let _ = client.cancel().await;
    report
}

/// The step whose `what` starts with `prefix`.
fn step<'a>(report: &'a SweepReport, prefix: &str) -> &'a sweep::SweepStep {
    report
        .steps
        .iter()
        .find(|step| step.what.starts_with(prefix))
        .unwrap_or_else(|| {
            panic!(
                "no step for {prefix:?}; the sweep drove {:?}",
                report.steps.iter().map(|s| &s.what).collect::<Vec<_>>()
            )
        })
}

#[tokio::test]
async fn the_sweep_drives_every_operation_it_claims_to() {
    let report = swept().await;
    // Each of these is a clause group that reports *not observed* without it,
    // which is what the sweep was built to fix. Naming them individually
    // rather than asserting a count: a count passes while the wrong step is
    // missing.
    for prefix in [
        "resources/list",
        "resources/read test://static-text",
        "resources/read test://static-binary",
        "resources/templates/list",
        // The template's `{id}` substituted — the step that proves
        // `substitute` is wired in, not merely unit-tested.
        "resources/read test://template/1/data",
        "prompts/list",
        "prompts/get test_simple_prompt",
        "prompts/get test_prompt_with_arguments",
        "prompts/get test_prompt_with_embedded_resource",
        "prompts/get test_prompt_with_image",
        "completion/complete",
    ] {
        assert!(
            step(&report, prefix).outcome.is_ok(),
            "{prefix} must succeed: {:?}",
            step(&report, prefix).outcome
        );
    }
}

#[tokio::test]
async fn the_one_deliberate_miss_is_the_only_failure() {
    // The read of a URI the catalog does not contain is the fixture that gives
    // the error-shape and error-code clauses something to judge. If it ever
    // stops failing, the sweep has stopped carrying them; if anything *else*
    // fails, the sweep is reporting the server for the client's mistake.
    let report = swept().await;
    assert_eq!(report.errors(), 1, "{:?}", report.steps);
    let absent = step(&report, "resources/read test://no-such-resource");
    assert!(absent.outcome.is_err(), "{absent:?}");
}

#[tokio::test]
async fn a_prompt_argument_naming_a_resource_gets_one_the_server_listed() {
    // The everything server embeds this value verbatim, so a placeholder would
    // put a URI with no RFC 3986 scheme in the recording — a RES-004 finding
    // the client manufactured and the server would be reported for. The
    // discovered URI is what prevents that, and it only works if the resource
    // sweep runs *first* and returns something.
    let report = swept().await;
    let embedded = step(&report, "prompts/get test_prompt_with_embedded_resource");
    assert!(
        embedded.outcome.is_ok(),
        "the embedded-resource prompt must succeed: {embedded:?}"
    );
    // Two messages: the embedded resource, then the instruction about it. One
    // would mean the server took a different branch than the fixture expects.
    assert_eq!(
        embedded.outcome.as_deref(),
        Ok("2 message(s)"),
        "{embedded:?}"
    );
}

#[tokio::test]
async fn resources_are_swept_before_prompts_need_them() {
    // The ordering is load-bearing rather than cosmetic; asserting it here
    // means a refactor that reorders `run` fails this test instead of quietly
    // reintroducing the placeholder URI.
    let report = swept().await;
    let index = |prefix: &str| {
        report
            .steps
            .iter()
            .position(|step| step.what.starts_with(prefix))
            .unwrap_or_else(|| panic!("no step for {prefix}"))
    };
    assert!(
        index("resources/list") < index("prompts/get"),
        "{:?}",
        report.steps.iter().map(|s| &s.what).collect::<Vec<_>>()
    );
    assert!(
        index("prompts/list") < index("completion/complete"),
        "completion addresses a prompt the listing named"
    );
}
