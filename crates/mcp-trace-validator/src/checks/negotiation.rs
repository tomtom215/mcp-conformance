// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The negotiated-capability usage check (`LIFE-009`): "Both parties MUST: Only use
//! capabilities that were successfully negotiated".
//!
//! A method table maps each capability-gated request and notification to the
//! declaration its use depends on. The check abstains when the trace carries no
//! `initialize` result (nothing was negotiated *or* the trace is truncated — the
//! handshake checks own that finding); it judges only sessions whose negotiation
//! outcome is visible.

use mcp_conformance_core::capability::CapabilityParty;
use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;

use super::FindingSink;
use super::support::{client_capability, server_capability};
use crate::context::TraceContext;

/// Capability-gated methods of `2025-11-25`: who sends them, and which declared
/// capability their use depends on. Ungated methods (`initialize`, `ping`,
/// cancellation, progress) are deliberately absent.
const GATED_METHODS: &[(Direction, &str, CapabilityParty, &[&str])] = &[
    (
        Direction::ClientToServer,
        "tools/list",
        CapabilityParty::Server,
        &["tools"],
    ),
    (
        Direction::ClientToServer,
        "tools/call",
        CapabilityParty::Server,
        &["tools"],
    ),
    (
        Direction::ClientToServer,
        "resources/list",
        CapabilityParty::Server,
        &["resources"],
    ),
    (
        Direction::ClientToServer,
        "resources/read",
        CapabilityParty::Server,
        &["resources"],
    ),
    (
        Direction::ClientToServer,
        "resources/templates/list",
        CapabilityParty::Server,
        &["resources"],
    ),
    (
        Direction::ClientToServer,
        "resources/subscribe",
        CapabilityParty::Server,
        &["resources", "subscribe"],
    ),
    (
        Direction::ClientToServer,
        "resources/unsubscribe",
        CapabilityParty::Server,
        &["resources", "subscribe"],
    ),
    (
        Direction::ClientToServer,
        "prompts/list",
        CapabilityParty::Server,
        &["prompts"],
    ),
    (
        Direction::ClientToServer,
        "prompts/get",
        CapabilityParty::Server,
        &["prompts"],
    ),
    (
        Direction::ClientToServer,
        "completion/complete",
        CapabilityParty::Server,
        &["completions"],
    ),
    (
        Direction::ClientToServer,
        "logging/setLevel",
        CapabilityParty::Server,
        &["logging"],
    ),
    (
        Direction::ClientToServer,
        "notifications/roots/list_changed",
        CapabilityParty::Client,
        &["roots", "listChanged"],
    ),
    (
        Direction::ServerToClient,
        "notifications/tools/list_changed",
        CapabilityParty::Server,
        &["tools", "listChanged"],
    ),
    (
        Direction::ServerToClient,
        "notifications/resources/list_changed",
        CapabilityParty::Server,
        &["resources", "listChanged"],
    ),
    (
        Direction::ServerToClient,
        "notifications/resources/updated",
        CapabilityParty::Server,
        &["resources", "subscribe"],
    ),
    (
        Direction::ServerToClient,
        "notifications/prompts/list_changed",
        CapabilityParty::Server,
        &["prompts", "listChanged"],
    ),
    (
        Direction::ServerToClient,
        "notifications/message",
        CapabilityParty::Server,
        &["logging"],
    ),
    (
        Direction::ServerToClient,
        "sampling/createMessage",
        CapabilityParty::Client,
        &["sampling"],
    ),
    (
        Direction::ServerToClient,
        "elicitation/create",
        CapabilityParty::Client,
        &["elicitation"],
    ),
    (
        Direction::ServerToClient,
        "roots/list",
        CapabilityParty::Client,
        &["roots"],
    ),
];

/// `LIFE-009`: every capability-gated message must ride on a declared capability.
pub(super) fn negotiated_capabilities_only(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        let method = match kind {
            MessageKind::Request { method, .. } | MessageKind::Notification { method } => *method,
            _ => continue,
        };
        let gate = GATED_METHODS
            .iter()
            .find(|(direction, gated, ..)| *direction == event.direction && *gated == method);
        let Some((_, _, party, path)) = gate else {
            continue;
        };
        let declared = match party {
            CapabilityParty::Server => server_capability(context, path),
            CapabilityParty::Client => client_capability(context, path),
        };
        if declared == Some(false) {
            let owner = match party {
                CapabilityParty::Server => "server",
                CapabilityParty::Client => "client",
            };
            sink.push(
                Some(event.seq),
                format!(
                    "{method:?} uses the {owner} capability {}, which was not negotiated in this session",
                    path.join(".")
                ),
            );
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::checks;
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};

    fn findings_for(trace: &str) -> Vec<String> {
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find("lifecycle.negotiated-capabilities-only")
            .unwrap()
            .run(&context)
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    fn handshake(client_capabilities: &str, server_capabilities: &str) -> String {
        let request = format!(
            r#"{{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{client_capabilities},"clientInfo":{{"name":"t","version":"0"}}}}}}}}"#
        );
        let result = format!(
            r#"{{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{server_capabilities},"serverInfo":{{"name":"s","version":"0"}}}}}}}}"#
        );
        let initialized = r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;
        format!("{request}\n{result}\n{initialized}")
    }

    #[test]
    fn flags_undeclared_sub_capability_but_not_declared_parent() {
        // resources declared without subscribe: read is fine, subscribe is not.
        let trace = format!(
            "{}\n{}\n{}",
            handshake("{}", r#"{"resources":{}}"#),
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"file:///a"}}}"#,
            r#"{"seq":4,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"method":"resources/subscribe","params":{"uri":"file:///a"}}}"#,
        );
        let findings = findings_for(&trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("resources.subscribe"), "{findings:?}");
    }

    #[test]
    fn judges_client_capabilities_for_server_initiated_traffic() {
        let trace = format!(
            "{}\n{}",
            handshake(r#"{"roots":{}}"#, "{}"),
            r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":"s1","method":"sampling/createMessage","params":{}}}"#,
        );
        let findings = findings_for(&trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].contains("client capability sampling"),
            "{findings:?}"
        );
    }

    #[test]
    fn direction_guard_keeps_wrong_way_messages_out_of_scope() {
        // A *server*-emitted tools/list request is not a client capability use; the
        // table must not match it (other checks own that weirdness).
        let trace = format!(
            "{}\n{}",
            handshake("{}", "{}"),
            r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":9,"method":"tools/list"}}"#,
        );
        assert!(findings_for(&trace).is_empty());
    }

    #[test]
    fn abstains_when_negotiation_is_invisible() {
        let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#;
        assert!(findings_for(trace).is_empty());
    }

    #[test]
    fn declared_capabilities_pass() {
        let trace = format!(
            "{}\n{}\n{}",
            handshake(r#"{"roots":{"listChanged":true}}"#, r#"{"logging":{}}"#),
            r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"x"}}}"#,
            r#"{"seq":4,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/roots/list_changed"}}"#,
        );
        assert!(findings_for(&trace).is_empty());
    }
}
