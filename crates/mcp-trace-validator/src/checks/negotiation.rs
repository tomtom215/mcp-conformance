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
//!
//! Three uses depend on a sub-capability that the method alone does not show, and
//! each is a MUST NOT in the `2025-11-25` text of the page that defines it:
//! an elicitation in a mode the client did not declare (`client/elicitation`:
//! "Servers MUST NOT send elicitation requests with modes that are not supported by
//! the client"), a sampling request carrying `tools` without `sampling.tools`
//! (`client/sampling`), and a task-augmented `tools/call` without the server's
//! `tasks.requests.tools.call` (`basic/utilities/tasks`). Those pages are not in
//! this revision's registry, so this clause is where the rules are judged.

use mcp_conformance_core::capability::CapabilityParty;
use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

use super::FindingSink;
use super::support::{Declaration, client_capability, server_capability};
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
        let declared = declaration(context, *party, path);
        if matches!(declared, Declaration::Unknowable) {
            // No initialize result, so neither side's declarations are in this
            // trace. The message is gated on something the capture cannot show,
            // which is not the same as riding on a capability nobody negotiated.
            continue;
        }
        // The subject is a capability-gated message in a session whose
        // declarations are readable; one that sent none put nothing to the
        // test, however long it ran.
        sink.examined();
        let (path, declared): (&[&str], Declaration) = match declared {
            Declaration::Withheld => (path, declared),
            _ => match sub_capability(context, event, method) {
                Some((sub_path, sub_declared)) => (sub_path, sub_declared),
                None => continue,
            },
        };
        if matches!(declared, Declaration::Withheld) {
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

fn declaration(context: &TraceContext<'_>, party: CapabilityParty, path: &[&str]) -> Declaration {
    match party {
        CapabilityParty::Server => server_capability(context, path),
        CapabilityParty::Client => client_capability(context, path),
    }
}

/// The sub-capability this use of `method` depends on, beyond the one the method
/// table names, with whether it was declared. The owning party is the method's.
fn sub_capability(
    context: &TraceContext<'_>,
    event: &TraceEvent,
    method: &str,
) -> Option<(&'static [&'static str], Declaration)> {
    let params = event.message_payload()?.get("params")?;
    match method {
        "elicitation/create" => {
            // `mode` defaults to form; `elicitation: {}` declares form mode only.
            if params.get("mode").and_then(Value::as_str) == Some("url") {
                let path: &[&str] = &["elicitation", "url"];
                return Some((path, client_capability(context, path)));
            }
            let path: &[&str] = &["elicitation", "form"];
            let declared = match client_capability(context, path) {
                Declaration::Withheld
                    if matches!(
                        client_capability(context, &["elicitation", "url"]),
                        Declaration::Withheld
                    ) =>
                {
                    Declaration::Declared
                }
                other => other,
            };
            Some((path, declared))
        }
        "sampling/createMessage" if params.get("tools").is_some() => {
            let path: &[&str] = &["sampling", "tools"];
            Some((path, client_capability(context, path)))
        }
        "tools/call" if params.get("task").is_some() => {
            let path: &[&str] = &["tasks", "requests", "tools", "call"];
            Some((path, server_capability(context, path)))
        }
        _ => None,
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
            .findings
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
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        let outcome = checks::find("lifecycle.negotiated-capabilities-only")
            .unwrap()
            .run(&context);
        assert!(outcome.findings.is_empty());
        // The half this test used to omit, and the reason it passed for as long
        // as the check reported *pass* here: an abstention and a pass both have
        // no findings, and only the subject count separates them.
        assert_eq!(
            outcome.subjects, 0,
            "a session with no handshake shows neither compliance nor violation"
        );
    }

    fn elicit(client_capabilities: &str, params: &str) -> Vec<String> {
        findings_for(&format!(
            "{}\n{{\"seq\":3,\"direction\":\"server-to-client\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":\"s1\",\"method\":\"elicitation/create\",\"params\":{params}}}}}",
            handshake(client_capabilities, "{}")
        ))
    }

    #[test]
    fn an_elicitation_mode_the_client_did_not_declare_is_a_finding() {
        let url = r#"{"mode":"url","message":"m","url":"https://example.com","elicitationId":"e"}"#;
        let form = r#"{"message":"m","requestedSchema":{"type":"object","properties":{}}}"#;
        // `elicitation: {}` is form mode only.
        let findings = elicit(r#"{"elicitation":{}}"#, url);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("elicitation.url"), "{findings:?}");
        assert!(elicit(r#"{"elicitation":{}}"#, form).is_empty());
        // A URL-only client was not offered form mode, the default.
        let findings = elicit(r#"{"elicitation":{"url":{}}}"#, form);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("elicitation.form"), "{findings:?}");
        assert!(elicit(r#"{"elicitation":{"url":{}}}"#, url).is_empty());
        assert!(elicit(r#"{"elicitation":{"form":{},"url":{}}}"#, form).is_empty());
        // No elicitation at all is still reported once, against the parent.
        let findings = elicit("{}", url);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0]
                .ends_with("capability elicitation, which was not negotiated in this session"),
            "{findings:?}"
        );
    }

    #[test]
    fn tool_enabled_sampling_needs_sampling_tools() {
        let request = r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":"s1","method":"sampling/createMessage","params":{"messages":[],"maxTokens":10,"tools":[{"name":"t","inputSchema":{"type":"object"}}]}}}"#;
        let findings = findings_for(&format!(
            "{}\n{request}",
            handshake(r#"{"sampling":{}}"#, "{}")
        ));
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("sampling.tools"), "{findings:?}");
        let declared = format!(
            "{}\n{request}",
            handshake(r#"{"sampling":{"tools":{}}}"#, "{}")
        );
        assert!(findings_for(&declared).is_empty());
        let plain = request.replace(
            r#","tools":[{"name":"t","inputSchema":{"type":"object"}}]"#,
            "",
        );
        assert!(
            findings_for(&format!(
                "{}\n{plain}",
                handshake(r#"{"sampling":{}}"#, "{}")
            ))
            .is_empty()
        );
    }

    #[test]
    fn a_task_augmented_call_needs_the_servers_task_support() {
        let call = r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"t","task":{"ttl":1000}}}}"#;
        let findings = findings_for(&format!("{}\n{call}", handshake("{}", r#"{"tools":{}}"#)));
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].contains("tasks.requests.tools.call"),
            "{findings:?}"
        );
        let declared = handshake(
            "{}",
            r#"{"tools":{},"tasks":{"requests":{"tools":{"call":{}}}}}"#,
        );
        assert!(findings_for(&format!("{declared}\n{call}")).is_empty());
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
