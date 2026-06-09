// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Structural classification of JSON-RPC 2.0 messages as MCP constrains them.
//!
//! MCP requires that "All messages between MCP clients and servers **MUST** follow the
//! JSON-RPC 2.0 specification" and then tightens the base rules (string-or-integer
//! request IDs, no `null` IDs, no batching). Classification here is deliberately
//! *lenient*: it answers "what is this message trying to be?" so that conformance
//! checks can report precise violations instead of refusing to look at malformed input.

use serde_json::Value;

/// The structural role of a single JSON-RPC message, as determined by which
/// combination of `method`, `id`, `result`, and `error` members is present.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageKind<'a> {
    /// A request: has `method` and an `id` member (whatever the `id`'s type).
    Request {
        /// The request's `method` member.
        method: &'a str,
        /// The request's `id` member, unvalidated — checks decide whether its type is
        /// permitted.
        id: &'a Value,
    },
    /// A notification: has `method` and no `id` member.
    Notification {
        /// The notification's `method` member.
        method: &'a str,
    },
    /// A response carrying `result`.
    Result {
        /// The response's `id` member, if present.
        id: Option<&'a Value>,
    },
    /// A response carrying `error`.
    Error {
        /// The response's `id` member, if present.
        id: Option<&'a Value>,
        /// The `error` member, unvalidated — checks decide whether its shape is
        /// permitted.
        error: &'a Value,
    },
    /// Not classifiable as any JSON-RPC message shape.
    Invalid {
        /// Human-readable reason the message could not be classified.
        reason: &'static str,
    },
}

/// Classifies a JSON value as a JSON-RPC message shape.
///
/// The `jsonrpc` version member is *not* inspected here — its value is a conformance
/// question (see requirement `BASE-008`), not a classification question.
#[must_use]
pub fn classify(payload: &Value) -> MessageKind<'_> {
    let Some(object) = payload.as_object() else {
        return MessageKind::Invalid {
            reason: "message is not a JSON object",
        };
    };
    let id = object.get("id");
    if let Some(method) = object.get("method") {
        let Some(method) = method.as_str() else {
            return MessageKind::Invalid {
                reason: "method member is not a string",
            };
        };
        return id.map_or(MessageKind::Notification { method }, |id| {
            MessageKind::Request { method, id }
        });
    }
    match (object.get("result"), object.get("error")) {
        (Some(_), Some(_)) => MessageKind::Invalid {
            reason: "message carries both result and error members",
        },
        (Some(_), None) => MessageKind::Result { id },
        (None, Some(error)) => MessageKind::Error { id, error },
        (None, None) => MessageKind::Invalid {
            reason: "message carries none of method, result, or error",
        },
    }
}

/// Returns `true` when a request/notification `method` is in the reserved
/// `notifications/` namespace, which MCP defines as one-way messages that
/// "MUST NOT include an ID".
#[must_use]
pub fn is_notification_method(method: &str) -> bool {
    method.starts_with("notifications/")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_request() {
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
        match classify(&message) {
            MessageKind::Request { method, id } => {
                assert_eq!(method, "tools/list");
                assert_eq!(id, &json!(1));
            }
            other => panic!("expected request, got {other:?}"),
        }
    }

    #[test]
    fn classifies_notification() {
        let message = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert_eq!(
            classify(&message),
            MessageKind::Notification {
                method: "notifications/initialized"
            }
        );
    }

    #[test]
    fn null_id_still_classifies_as_request() {
        // A `null` id is a *violation* (BASE-002), not an unclassifiable message;
        // classification must surface it so the check can point at it.
        let message = json!({"jsonrpc": "2.0", "id": null, "method": "tools/list"});
        assert!(matches!(classify(&message), MessageKind::Request { .. }));
    }

    #[test]
    fn classifies_result_and_error_responses() {
        let result = json!({"jsonrpc": "2.0", "id": 7, "result": {}});
        assert!(matches!(
            classify(&result),
            MessageKind::Result { id: Some(_) }
        ));

        let error = json!({"jsonrpc": "2.0", "id": 7, "error": {"code": -32600, "message": "x"}});
        assert!(matches!(classify(&error), MessageKind::Error { .. }));

        let error_without_id = json!({"jsonrpc": "2.0", "error": {"code": -32700, "message": "x"}});
        assert!(matches!(
            classify(&error_without_id),
            MessageKind::Error { id: None, .. }
        ));
    }

    #[test]
    fn rejects_unclassifiable_shapes() {
        for (payload, fragment) in [
            (json!([1, 2]), "not a JSON object"),
            (json!({"id": 1}), "none of method, result, or error"),
            (json!({"method": 42}), "method member is not a string"),
            (
                json!({"id": 1, "result": {}, "error": {}}),
                "both result and error",
            ),
        ] {
            match classify(&payload) {
                MessageKind::Invalid { reason } => {
                    assert!(reason.contains(fragment), "{reason} vs {fragment}");
                }
                other => panic!("expected invalid for {payload}, got {other:?}"),
            }
        }
    }

    #[test]
    fn notification_namespace_detection() {
        assert!(is_notification_method("notifications/initialized"));
        assert!(is_notification_method("notifications/cancelled"));
        assert!(!is_notification_method("tools/list"));
        assert!(!is_notification_method("notification/initialized"));
    }
}
