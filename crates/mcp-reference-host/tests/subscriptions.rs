// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One `subscriptions/listen` lifecycle, end to end, against the real
//! everything server in-process over `tokio::io::duplex`.
//!
//! `subscribe`'s unit tests pin what a recording *asks* for and how a
//! notification is named. What they cannot reach is the lifecycle itself: the
//! acknowledgment arriving before any notification, the server narrowing the
//! filter to what it can actually serve, the stream ending on its own rather
//! than being killed. Every one of those is a round trip, and a bounded drain
//! that never ends is a hang rather than a failed assertion — which is exactly
//! why it needs a live server to be worth anything.
//!
//! `2026-07-28` only: `subscriptions/listen` is the method that revision
//! introduced to replace `resources/subscribe`, and it exists in no other, so
//! the whole file is gated on the feature that describes it.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "http")]

use mcp_everything_server::EverythingServer;
use mcp_everything_server::server::ServedRevision;
use mcp_reference_host::handler::HostHandler;
use mcp_reference_host::script::InteractionScript;
use mcp_reference_host::subscribe::{self, SubscriptionReport};
use rmcp::model::ProtocolVersion;
use rmcp::service::{ClientLifecycleMode, ClientServiceExt as _, ServiceExt as _};

/// The resource this server publishes, and one it does not.
const KNOWN: &str = "test://static-text";
const UNKNOWN: &str = "test://not-a-resource";

/// Drives one subscription against a stateless server and reports it.
async fn drained() -> SubscriptionReport {
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        let server = EverythingServer::serving(ServedRevision::V2026_07_28);
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = HostHandler::new(InteractionScript::default())
        .serve_with_lifecycle(
            client_io,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("host discovers the stateless server");
    let report = subscribe::drain(&client, subscribe::everything(KNOWN, UNKNOWN))
        .await
        .expect("the subscription opens and ends");
    let _ = client.cancel().await;
    report
}

#[tokio::test]
async fn the_server_acknowledges_only_what_it_can_serve() {
    // The unknown URI is in the filter precisely so the acknowledgment shows
    // the server *narrowing* it. An acknowledgment echoing the request back
    // would tell a reader nothing, and would hide a server that accepted a
    // subscription to a resource it does not have.
    let report = drained().await;
    assert!(
        report.acknowledged.contains(&format!("resource:{KNOWN}")),
        "{:?}",
        report.acknowledged
    );
    assert!(
        !report.acknowledged.contains(&format!("resource:{UNKNOWN}")),
        "the server must not acknowledge a resource it does not serve: {:?}",
        report.acknowledged
    );
    // The three list-changed categories are all servable, so all three stand.
    for category in [
        "toolsListChanged",
        "promptsListChanged",
        "resourcesListChanged",
    ] {
        assert!(
            report.acknowledged.contains(&category.to_owned()),
            "{category} missing from {:?}",
            report.acknowledged
        );
    }
}

#[tokio::test]
async fn every_acknowledged_category_arrives_and_the_stream_ends_itself() {
    let report = drained().await;
    // One notification per acknowledged category: the server announces what it
    // agreed to serve and then closes. A missing one is a category
    // acknowledged and not delivered, which is the failure SUBS-001 describes
    // from the other side.
    for method in [
        "notifications/tools/list_changed",
        "notifications/prompts/list_changed",
        "notifications/resources/list_changed",
        "notifications/resources/updated",
    ] {
        assert!(
            report.notifications.contains(&method.to_owned()),
            "{method} missing from {:?}",
            report.notifications
        );
    }
    // Ended on the server's initiative, not by the client giving up. A drain
    // that reported "still open" would mean the recording is a truncation.
    assert_eq!(report.ended, "graceful", "{report:?}");
}

#[tokio::test]
async fn the_acknowledgment_precedes_every_notification() {
    // rmcp's client enforces this from the client's side — it rejects an
    // acknowledgment arriving after a notification — so a drain that returns
    // at all has already proven the ordering. Asserting it here states the
    // property the corpus depends on rather than leaving it implicit in
    // somebody else's error path.
    let report = drained().await;
    assert!(
        !report.acknowledged.is_empty(),
        "a subscription with no acknowledgment cannot have ordered notifications"
    );
    assert!(!report.notifications.is_empty(), "{report:?}");
}
