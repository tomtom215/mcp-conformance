// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The host's [`rmcp::ClientHandler`].
//!
//! A thin shell over pure decision functions driven by an
//! [`InteractionScript`], with an event log making every server-initiated
//! interaction assertable (and, later, traceable).
//!
//! URL-mode elicitation follows the `2025-11-25` contract: `accept` records
//! the user's consent to navigate — the interaction itself is out of band —
//! and completion arrives (if ever) as `notifications/elicitation/complete`,
//! whose unknown or already-completed `elicitationId`s a client **MUST**
//! ignore. The pending-id set enforces that rule and the event log proves it.

// SEP-2577 forward-deprecates Roots, Sampling and Logging. They remain fully
// functional and REQUIRED on the `2025-11-25` surface this crate implements
// and the official suite exercises, so rmcp 3.x's deprecation attributes fire
// on correct code — here, Sampling and Roots. Scoped to this module, never the crate:
// a blanket allow would also hide a deprecation that genuinely matters. The
// honest cost is that a *different* future deprecation in this module would
// be silenced too. Retires when the `2025-11-25` surface does.
#![allow(deprecated)]
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use rmcp::model::{
    ClientCapabilities, ClientInfo, CreateMessageRequestParams, CreateMessageResult,
    ElicitRequestParams, ElicitResult, ElicitationAction, ListRootsResult, SamplingMessage,
};
use rmcp::service::{NotificationContext, RequestContext, RoleClient};

use crate::script::{
    ElicitationPolicy, InteractionScript, UrlElicitationPolicy, defaults_from_schema,
};

/// One observed server-initiated interaction, in arrival order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostEvent {
    /// A `sampling/createMessage` request was answered from the script.
    SamplingAnswered,
    /// A form-mode elicitation was answered with this action.
    FormElicitationAnswered(&'static str),
    /// A URL-mode elicitation was answered; for `accept`, consent to navigate
    /// was recorded and the id became pending.
    UrlElicitationAnswered {
        /// The server-assigned `elicitationId`.
        elicitation_id: String,
        /// The action taken (`accept` or `decline`).
        action: &'static str,
    },
    /// A completion notification matched a pending id, which is now spent.
    UrlElicitationCompleted(String),
    /// A completion notification named an unknown or already-completed id
    /// and was ignored, as the spec requires of clients.
    UnknownElicitationCompletionIgnored(String),
    /// `roots/list` was answered from the script.
    RootsListed,
    /// An `elicitation/create` arrived in a mode this host does not model.
    /// `2025-11-25` defines exactly form and URL mode; `ElicitRequestParams`
    /// became `#[non_exhaustive]` in rmcp 3.x, so a future mode reaches here
    /// and is declined rather than guessed at.
    UnsupportedElicitationModeDeclined,
}

/// Scripted client handler. Cloning shares the event log and pending-id set,
/// so a test keeps one handle while the client service owns another.
#[derive(Debug, Clone, Default)]
pub struct HostHandler {
    script: InteractionScript,
    events: Arc<Mutex<Vec<HostEvent>>>,
    pending_elicitations: Arc<Mutex<BTreeSet<String>>>,
}

impl HostHandler {
    /// A handler answering from `script`.
    #[must_use]
    pub fn new(script: InteractionScript) -> Self {
        Self {
            script,
            events: Arc::default(),
            pending_elicitations: Arc::default(),
        }
    }

    /// Everything the server asked of this host so far, in order.
    #[must_use]
    pub fn events(&self) -> Vec<HostEvent> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, event: HostEvent) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }
}

/// The scripted answer to one elicitation request, plus the event recording
/// it. Pure: policy in, wire result out — testable without a transport.
fn answer_elicitation(
    script: &InteractionScript,
    params: &ElicitRequestParams,
) -> (ElicitResult, HostEvent, Option<String>) {
    match params {
        ElicitRequestParams::FormElicitationParams {
            requested_schema, ..
        } => {
            let (action, content, label) = match &script.elicitation {
                ElicitationPolicy::AcceptWithDefaults => (
                    ElicitationAction::Accept,
                    Some(serde_json::Value::Object(defaults_from_schema(
                        requested_schema,
                    ))),
                    "accept",
                ),
                ElicitationPolicy::AcceptWith(content) => (
                    ElicitationAction::Accept,
                    Some(serde_json::Value::Object(content.clone())),
                    "accept",
                ),
                ElicitationPolicy::Decline => (ElicitationAction::Decline, None, "decline"),
                ElicitationPolicy::Cancel => (ElicitationAction::Cancel, None, "cancel"),
            };
            let mut result = ElicitResult::new(action);
            result.content = content;
            (result, HostEvent::FormElicitationAnswered(label), None)
        }
        ElicitRequestParams::UrlElicitationParams { elicitation_id, .. } => {
            let (action, label, pending) = match script.url_elicitation {
                UrlElicitationPolicy::AcceptConsent => (
                    ElicitationAction::Accept,
                    "accept",
                    Some(elicitation_id.clone()),
                ),
                UrlElicitationPolicy::Decline => (ElicitationAction::Decline, "decline", None),
            };
            (
                ElicitResult::new(action),
                HostEvent::UrlElicitationAnswered {
                    elicitation_id: elicitation_id.clone(),
                    action: label,
                },
                pending,
            )
        }
        // Declining is the only honest answer to a mode whose semantics this
        // host does not implement: accepting would assert consent it never
        // obtained. See `HostEvent::UnsupportedElicitationModeDeclined`.
        _ => (
            ElicitResult::new(ElicitationAction::Decline),
            HostEvent::UnsupportedElicitationModeDeclined,
            None,
        ),
    }
}

/// Disposition of one `notifications/elicitation/complete`: completing a
/// pending id spends it; anything else is ignored per the spec's client MUST.
fn note_completion(pending: &mut BTreeSet<String>, elicitation_id: &str) -> HostEvent {
    if pending.remove(elicitation_id) {
        HostEvent::UrlElicitationCompleted(elicitation_id.to_owned())
    } else {
        HostEvent::UnknownElicitationCompletionIgnored(elicitation_id.to_owned())
    }
}

impl rmcp::ClientHandler for HostHandler {
    fn get_info(&self) -> ClientInfo {
        let mut info = ClientInfo::default();
        // Capability honesty: declared because each is answered above —
        // sampling and elicitation by the script, roots by the script's list.
        info.capabilities = ClientCapabilities::builder()
            .enable_sampling()
            .enable_elicitation()
            .enable_roots()
            .build();
        info
    }

    async fn create_message(
        &self,
        _params: CreateMessageRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<CreateMessageResult, rmcp::ErrorData> {
        self.record(HostEvent::SamplingAnswered);
        Ok(CreateMessageResult::new(
            SamplingMessage::assistant_text(self.script.sampling_reply.clone()),
            self.script.sampling_model.clone(),
        ))
    }

    async fn create_elicitation(
        &self,
        params: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, rmcp::ErrorData> {
        let (result, event, pending) = answer_elicitation(&self.script, &params);
        if let Some(id) = pending {
            self.pending_elicitations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id);
        }
        self.record(event);
        Ok(result)
    }

    async fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, rmcp::ErrorData> {
        self.record(HostEvent::RootsListed);
        Ok(ListRootsResult::new(self.script.roots.clone()))
    }

    /// rmcp 3.x removed the typed `on_url_elicitation_notification_complete`
    /// hook along with the `2026-07-28` deletion of
    /// `notifications/elicitation/complete` (register 1.5d Minor #11). The
    /// notification is still part of `2025-11-25`, which this host speaks, so
    /// it is received through the generic seam and dispatched on the method
    /// name — the mirror of how the everything server now sends it. Anything
    /// else that arrives here is ignored, exactly as the default hook did.
    async fn on_custom_notification(
        &self,
        notification: rmcp::model::CustomNotification,
        _context: NotificationContext<RoleClient>,
    ) {
        // The literal `2025-11-25` method name, deliberately not rmcp's
        // `ElicitationResponseNotificationMethod` constant: in rmcp 3.x that
        // constant is `notifications/elicitation/response`, a *different*
        // notification. Binding to it would leave this host silently deaf to
        // the completion it is meant to observe.
        if notification.method != "notifications/elicitation/complete" {
            return;
        }
        let Some(elicitation_id) = notification
            .params
            .as_ref()
            .and_then(|params| params.get("elicitationId"))
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let event = {
            let mut pending = self
                .pending_elicitations
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            note_completion(&mut pending, elicitation_id)
        };
        self.record(event);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn form_params(schema: &serde_json::Value) -> ElicitRequestParams {
        serde_json::from_value(serde_json::json!({
            "mode": "form",
            "message": "fill the form",
            "requestedSchema": schema,
        }))
        .unwrap()
    }

    fn url_params(id: &str) -> ElicitRequestParams {
        serde_json::from_value(serde_json::json!({
            "mode": "url",
            "message": "continue in the browser",
            "url": "https://mcp.example/ui/key",
            "elicitationId": id,
        }))
        .unwrap()
    }

    #[test]
    fn form_accept_with_defaults_fills_the_schema_defaults() {
        let script = InteractionScript::default();
        let params = form_params(&serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string", "default": "John Doe" } },
            "required": []
        }));
        let (result, event, pending) = answer_elicitation(&script, &params);
        assert_eq!(result.action, ElicitationAction::Accept);
        assert_eq!(
            result.content,
            Some(serde_json::json!({"name": "John Doe"}))
        );
        assert_eq!(event, HostEvent::FormElicitationAnswered("accept"));
        assert_eq!(pending, None, "form mode never creates a pending id");
    }

    #[test]
    fn form_decline_and_cancel_carry_no_content() {
        for (policy, action, label) in [
            (
                ElicitationPolicy::Decline,
                ElicitationAction::Decline,
                "decline",
            ),
            (
                ElicitationPolicy::Cancel,
                ElicitationAction::Cancel,
                "cancel",
            ),
        ] {
            let script = InteractionScript {
                elicitation: policy,
                ..InteractionScript::default()
            };
            let (result, event, _) = answer_elicitation(
                &script,
                &form_params(&serde_json::json!({
                    "type": "object", "properties": {}, "required": []
                })),
            );
            assert_eq!(result.action, action);
            assert_eq!(result.content, None);
            assert_eq!(event, HostEvent::FormElicitationAnswered(label));
        }
    }

    #[test]
    fn url_consent_accepts_without_content_and_marks_pending() {
        let script = InteractionScript::default();
        let (result, event, pending) = answer_elicitation(&script, &url_params("e-1"));
        assert_eq!(result.action, ElicitationAction::Accept);
        assert_eq!(
            result.content, None,
            "URL mode carries no content: accept records consent only"
        );
        assert_eq!(
            event,
            HostEvent::UrlElicitationAnswered {
                elicitation_id: "e-1".to_owned(),
                action: "accept",
            }
        );
        assert_eq!(pending.as_deref(), Some("e-1"));
    }

    #[test]
    fn url_decline_leaves_nothing_pending() {
        let script = InteractionScript {
            url_elicitation: UrlElicitationPolicy::Decline,
            ..InteractionScript::default()
        };
        let (result, _, pending) = answer_elicitation(&script, &url_params("e-2"));
        assert_eq!(result.action, ElicitationAction::Decline);
        assert_eq!(pending, None);
    }

    #[test]
    fn completion_notifications_spend_known_ids_and_ignore_the_rest() {
        let mut pending: BTreeSet<String> = ["e-1".to_owned()].into();
        // Unknown id: ignored (the spec's client MUST).
        assert_eq!(
            note_completion(&mut pending, "never-issued"),
            HostEvent::UnknownElicitationCompletionIgnored("never-issued".to_owned())
        );
        assert!(
            pending.contains("e-1"),
            "unknown ids must not disturb state"
        );
        // Known id: completed and spent.
        assert_eq!(
            note_completion(&mut pending, "e-1"),
            HostEvent::UrlElicitationCompleted("e-1".to_owned())
        );
        // Already-completed id: now ignored.
        assert_eq!(
            note_completion(&mut pending, "e-1"),
            HostEvent::UnknownElicitationCompletionIgnored("e-1".to_owned())
        );
    }
}
