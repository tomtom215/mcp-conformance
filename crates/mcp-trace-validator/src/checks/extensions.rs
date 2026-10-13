// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the official extensions' clauses (ADR-0023).
//!
//! An extension's clauses are judged like any other and reported apart: the
//! engine puts their rows in the report's `extensions`, whose totals the verdict
//! never reads. The checks here are ordinary checks; what makes them extension
//! checks is only which entries name them.
//!
//! **Every subject here is extension traffic.** A check counts a subject only
//! where the session carried something the extension defines — a task result, a
//! `tasks/*` or `skills/*` call, a skill entry, a `skill://` resource — never a
//! core message. That is what keeps an extension's report off every recording
//! that never touched it: the engine lists an extension only once one of its
//! rows judged something, and a check that counted core traffic would list it
//! everywhere.

use serde_json::Value;

pub(super) mod skills;
pub(super) mod tasks;
#[cfg(test)]
mod testkit;

/// The `_meta` key carrying a request's declared client capabilities.
const CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

/// Whether a request's `params` declare `extension` in the client capabilities
/// its `_meta` carries — the per-request opt-in of this revision.
///
/// A declaration is present when the key resolves to something neither `false`
/// nor `null`, the reading ADR-0006 gives every capability.
pub(super) fn client_declares(params: Option<&Value>, extension: &str) -> bool {
    params
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get(CLIENT_CAPABILITIES))
        .and_then(|capabilities| capabilities.get("extensions"))
        .and_then(|extensions| extensions.get(extension))
        .is_some_and(|settings| !settings.is_null() && settings.as_bool() != Some(false))
}

/// The JSON-RPC error code of a response event, when it is an error.
pub(super) fn error_code(response: &mcp_conformance_core::trace::TraceEvent) -> Option<i64> {
    response
        .message_payload()?
        .get("error")?
        .get("code")?
        .as_i64()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::client_declares;

    #[test]
    fn a_declaration_is_any_value_but_false_or_null() {
        let params = |settings: serde_json::Value| {
            json!({"_meta": {"io.modelcontextprotocol/clientCapabilities":
                {"extensions": {"io.modelcontextprotocol/tasks": settings}}}})
        };
        assert!(client_declares(
            Some(&params(json!({}))),
            "io.modelcontextprotocol/tasks"
        ));
        assert!(client_declares(
            Some(&params(json!(true))),
            "io.modelcontextprotocol/tasks"
        ));
        assert!(!client_declares(
            Some(&params(json!(false))),
            "io.modelcontextprotocol/tasks"
        ));
        assert!(!client_declares(
            Some(&params(json!(null))),
            "io.modelcontextprotocol/tasks"
        ));
        assert!(!client_declares(
            Some(&params(json!({}))),
            "io.modelcontextprotocol/skills"
        ));
        assert!(!client_declares(
            Some(&json!({})),
            "io.modelcontextprotocol/tasks"
        ));
        assert!(!client_declares(None, "io.modelcontextprotocol/tasks"));
    }
}
