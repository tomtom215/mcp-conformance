// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `io.modelcontextprotocol/serverInfo` on every result (BASE-037).
//!
//! `2026-07-28` (`basic/index#_meta`): "Servers SHOULD include the following
//! `io.modelcontextprotocol/*` field in every result's `meta` … to identify
//! themselves without relying on any prior connection state". rmcp 3.5.1 sets
//! it on the two results it builds itself — `server/discover` (through
//! `DiscoverResult::from_server_info`) and `subscriptions/listen` — and on no
//! other, so a handler that wants the rest has to say so.
//!
//! **Why here, and not in a wrapping service.** A `Service<RoleServer>` around
//! the handler would be one seam for every result, but rmcp's
//! `StreamableHttpService` accepts only an `S: ServerHandler`, and a wrapper
//! implementing `Service` directly is not one (the same type bound that keeps
//! `StatelessEnvelope` off the HTTP transport). A wrapper implementing
//! `ServerHandler` would have to forward every one of the trait's methods by
//! hand, and a method it forgot would silently fall back to rmcp's default.
//! So the field is applied where [`super::caching`]'s hints are: at each
//! handler's return, through one trait, read from the same served revision.
//! The tap and the host's recorder both sit outside the handler, so what they
//! record is what the client receives.
//!
//! **What it does not cover.** `logging/setLevel`, `ping`,
//! `resources/subscribe` and `resources/unsubscribe` answer with rmcp's
//! `EmptyResult`, which has no `_meta` to carry the field. At `2026-07-28` rmcp
//! itself answers the last three `-32601` (they are legacy-lifecycle methods).
//! `logging/setLevel` does not exist at that revision, yet rmcp still
//! dispatches it, so the server's own handler refuses it there with `-32601`
//! rather than answer `{}`.

use rmcp::model::{
    CallToolResponse, CallToolResult, CompleteResult, GetPromptResponse, GetPromptResult,
    Implementation, InputRequiredResult, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, MetaObject, ReadResourceResponse, ReadResourceResult,
};
use serde_json::Value;

use super::ServedRevision;

/// The result `_meta` key a server names itself with.
pub(super) const SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// A result that can name the server that produced it.
pub(super) trait Identified: Sized {
    /// This result with `server` under [`SERVER_INFO`] in its `_meta`.
    fn named(self, server: &Value) -> Self;
}

/// Puts `server` in `meta`, keeping a value a handler already set there.
fn stamp(meta: &mut Option<MetaObject>, server: &Value) {
    meta.get_or_insert_default()
        .0
        .entry(SERVER_INFO)
        .or_insert_with(|| server.clone());
}

macro_rules! identified {
    ($($result:ty),+ $(,)?) => {
        $(impl Identified for $result {
            fn named(mut self, server: &Value) -> Self {
                stamp(&mut self.meta, server);
                self
            }
        })+
    };
}

identified!(
    CallToolResult,
    CompleteResult,
    GetPromptResult,
    InputRequiredResult,
    ListPromptsResult,
    ListResourceTemplatesResult,
    ListResourcesResult,
    ListToolsResult,
    ReadResourceResult,
);

// The MRTR unions: either arm is a result the client receives. Their
// `#[non_exhaustive]` catch-alls pass a future variant through unnamed rather
// than guessing at its shape — the integration test that walks every result
// kind is what would notice it.
impl Identified for CallToolResponse {
    fn named(self, server: &Value) -> Self {
        match self {
            Self::Complete(result) => Self::Complete(result.named(server)),
            Self::InputRequired(result) => Self::InputRequired(result.named(server)),
            Self::Task(mut result) => {
                stamp(&mut result.meta, server);
                Self::Task(result)
            }
            other => other,
        }
    }
}

impl Identified for GetPromptResponse {
    fn named(self, server: &Value) -> Self {
        match self {
            Self::Complete(result) => Self::Complete(result.named(server)),
            Self::InputRequired(result) => Self::InputRequired(result.named(server)),
            other => other,
        }
    }
}

impl Identified for ReadResourceResponse {
    fn named(self, server: &Value) -> Self {
        match self {
            Self::Complete(result) => Self::Complete(result.named(server)),
            Self::InputRequired(result) => Self::InputRequired(result.named(server)),
            other => other,
        }
    }
}

/// `result`, naming `server` when `revision` asks every result to.
///
/// An [`Implementation`] that will not serialize leaves the result as it was:
/// the field is a SHOULD, and an answer without it beats no answer.
pub(super) fn applied<T: Identified>(
    revision: ServedRevision,
    server: &Implementation,
    result: T,
) -> T {
    if !revision.names_itself_on_results() {
        return result;
    }
    match serde_json::to_value(server) {
        Ok(server) => result.named(&server),
        Err(_) => result,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use rmcp::model::{CompletionInfo, ListToolsResult};

    use super::*;

    fn server() -> Implementation {
        Implementation::new("mcp-everything-server", "0.0.0")
    }

    fn named_in(meta: Option<&MetaObject>) -> Option<&Value> {
        meta.and_then(|meta| meta.0.get(SERVER_INFO))
    }

    #[test]
    fn the_older_revision_names_nothing() {
        let result = applied(
            ServedRevision::V2025_11_25,
            &server(),
            ListToolsResult::with_all_items(Vec::new()),
        );
        assert!(result.meta.is_none(), "{:?}", result.meta);
    }

    #[test]
    fn the_stateless_revision_names_the_server_by_its_identity() {
        let result = applied(
            ServedRevision::V2026_07_28,
            &server(),
            CompleteResult::new(CompletionInfo::with_all_values(Vec::new()).unwrap()),
        );
        assert_eq!(
            named_in(result.meta.as_ref()),
            Some(&serde_json::to_value(server()).unwrap())
        );
    }

    #[test]
    fn both_arms_of_an_mrtr_union_are_named() {
        let complete = applied(
            ServedRevision::V2026_07_28,
            &server(),
            CallToolResponse::from(CallToolResult::success(Vec::new())),
        );
        let CallToolResponse::Complete(complete) = complete else {
            panic!("the arm changed")
        };
        assert!(named_in(complete.meta.as_ref()).is_some());
        let asking = InputRequiredResult::from_request_state("round-1");
        let asking = applied(
            ServedRevision::V2026_07_28,
            &server(),
            GetPromptResponse::from(asking),
        );
        let GetPromptResponse::InputRequired(asking) = asking else {
            panic!("the arm changed")
        };
        assert!(named_in(asking.meta.as_ref()).is_some());
    }

    #[test]
    fn a_task_handle_and_a_read_are_named_too() {
        // This server materializes no tasks, but the arm is a result a client
        // would receive, so it is named like the others.
        let task = rmcp::model::Task::new(
            "t-1",
            rmcp::model::TaskStatus::Working,
            "1970-01-01T00:00:00Z",
            "1970-01-01T00:00:00Z",
        );
        let named = applied(
            ServedRevision::V2026_07_28,
            &server(),
            CallToolResponse::Task(rmcp::model::CreateTaskResult::new(task)),
        );
        let CallToolResponse::Task(named) = named else {
            panic!("the arm changed")
        };
        assert!(named_in(named.meta.as_ref()).is_some());
        let read = applied(
            ServedRevision::V2026_07_28,
            &server(),
            ReadResourceResponse::from(ReadResourceResult::new(Vec::new())),
        );
        let ReadResourceResponse::Complete(read) = read else {
            panic!("the arm changed")
        };
        assert!(named_in(read.meta.as_ref()).is_some());
    }

    #[test]
    fn a_value_the_handler_set_is_kept_and_other_keys_survive() {
        let mut meta = MetaObject::new();
        meta.0.insert(SERVER_INFO.to_owned(), Value::from("own"));
        meta.0.insert("other".to_owned(), Value::from(1));
        let mut result = CallToolResult::success(Vec::new());
        result.meta = Some(meta);
        let result = applied(ServedRevision::V2026_07_28, &server(), result);
        let meta = result.meta.unwrap();
        assert_eq!(meta.0[SERVER_INFO], "own");
        assert_eq!(meta.0["other"], 1);
    }
}
