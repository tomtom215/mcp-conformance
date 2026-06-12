// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Server-initiated interactions through real wire dispatch: a probe server
//! whose `on_initialized` hook drives `roots/list`, a URL-mode elicitation,
//! and completion notifications at the connected host — the only way the
//! `ClientHandler` trait methods themselves are invoked, which is what the
//! diff-scoped mutation gate demands of them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mcp_reference_host::handler::{HostEvent, HostHandler};
use mcp_reference_host::script::InteractionScript;
use rmcp::model::{
    CreateElicitationRequestParams, ElicitationResponseNotificationParam, Root, ServerInfo,
};
use rmcp::service::{NotificationContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt as _};

/// What the probe observed from the host, plus a completion signal.
#[derive(Debug, Default)]
struct Observed {
    roots: Mutex<Option<Vec<Root>>>,
    done: tokio::sync::Notify,
}

/// A server that interrogates its client the moment it reports initialized:
/// list the roots, send one URL-mode elicitation, complete it by id, then
/// send a completion for an id that was never issued.
#[derive(Debug, Clone)]
struct ProbeServer {
    observed: Arc<Observed>,
}

impl ServerHandler for ProbeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let observed = Arc::clone(&self.observed);
        let peer = context.peer;
        // Drive from a task: the dispatch loop that called this hook is the
        // same one that must route the client's answers back.
        tokio::spawn(async move {
            if let Ok(result) = peer.list_roots().await {
                *observed.roots.lock().unwrap() = Some(result.roots);
            }
            let _ = peer
                .create_elicitation(CreateElicitationRequestParams::UrlElicitationParams {
                    meta: None,
                    message: "continue in the browser".to_owned(),
                    url: "https://mcp.example/ui/key".to_owned(),
                    elicitation_id: "probe-elic-1".to_owned(),
                })
                .await;
            let _ = peer
                .notify_url_elicitation_completed(ElicitationResponseNotificationParam::new(
                    "probe-elic-1",
                ))
                .await;
            let _ = peer
                .notify_url_elicitation_completed(ElicitationResponseNotificationParam::new(
                    "never-issued",
                ))
                .await;
            observed.done.notify_one();
        });
    }
}

#[tokio::test]
async fn roots_and_url_elicitation_flow_through_the_real_trait_dispatch() {
    let observed = Arc::new(Observed::default());
    let probe = ProbeServer {
        observed: Arc::clone(&observed),
    };
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(server) = probe.serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    let handler = HostHandler::new(InteractionScript::default());
    let client = handler
        .clone()
        .serve(client_io)
        .await
        .expect("host initializes");

    tokio::time::timeout(Duration::from_secs(10), observed.done.notified())
        .await
        .expect("the probe finishes its interrogation within 10s");

    // roots/list answered from the script — not an empty default.
    let roots = observed
        .roots
        .lock()
        .unwrap()
        .clone()
        .expect("roots/list was answered");
    assert_eq!(roots.len(), 1, "{roots:?}");
    assert_eq!(roots[0].uri, "file:///workspace/project");
    assert_eq!(roots[0].name.as_deref(), Some("project"));

    // The URL elicitation was consented to, its id spent exactly once, and
    // the never-issued id ignored — in wire order.
    let events = handler.events();
    let expected_tail = [
        HostEvent::RootsListed,
        HostEvent::UrlElicitationAnswered {
            elicitation_id: "probe-elic-1".to_owned(),
            action: "accept",
        },
        HostEvent::UrlElicitationCompleted("probe-elic-1".to_owned()),
        HostEvent::UnknownElicitationCompletionIgnored("never-issued".to_owned()),
    ];
    assert_eq!(
        events, expected_tail,
        "full interrogation observed in order"
    );

    client.cancel().await.expect("clean shutdown");
}
