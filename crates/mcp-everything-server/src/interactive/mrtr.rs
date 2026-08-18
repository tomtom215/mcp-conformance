// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Multi Round-Trip Requests (SEP-2322), and the one seam that hides which
//! era a tool is running in.
//!
//! `2026-07-28` forbids the mechanism the interactive tools were built on.
//! MRTR-001 requires server-to-client requests to go "using the MRTR pattern",
//! and the two transport pages state the prohibition directly: a server MUST
//! NOT send independent JSON-RPC requests on a response stream (TRAN-066) or
//! write them to `stdout` (TRAN-120). So `elicitation/create` cannot be *sent*
//! at this revision — it is *returned*, inside an `InputRequiredResult`, and
//! the client retries the original call carrying its answer.
//!
//! That inverts control: the older flow awaits a value mid-execution, the
//! newer one returns and is called again. [`Interaction`] is the shape that
//! lets one tool body express both — [`ask`] either produces the answer (the
//! legacy round trip, or a retry that already carries it) or produces the
//! result to hand back so the client can go and get it.
//!
//! **Both eras stay live.** `2025-11-25` has no MRTR, and the official suite's
//! elicitation and sampling scenarios drive the server-initiated form; the
//! branch here is the served revision, so neither surface can drift into the
//! other's shape.

use rmcp::RoleServer;
use rmcp::model::{
    CreateMessageRequest, CreateMessageRequestParams, ElicitRequest, ElicitRequestParams,
    ErrorData, InputRequest, InputRequests, InputRequiredResult,
};
use rmcp::service::RequestContext;
use serde_json::Value;

use crate::server::ServedRevision;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

/// The `inputRequests` key this server assigns.
///
/// MRTR-005 requires keys unique "within the scope of the request", and every
/// interactive tool here asks for exactly one thing — so one key is the whole
/// namespace, and a constant is more honest than a counter that would only
/// ever reach one.
const KEY: &str = "input";

/// What a tool needs from the client, and how far it got.
pub(super) enum Interaction {
    /// The answer, as the client's raw result. Opaque `Value` because that is
    /// what an MRTR retry carries — `InputResponses` is a map of JSON, since
    /// the heterogeneous client-result union cannot be a typed map value — and
    /// because giving the legacy path the same type keeps one formatter for
    /// both eras rather than two that could disagree.
    Answered(Value),
    /// Nothing yet: hand this back and the client will retry with the answer.
    Deferred(InputRequiredResult),
}

/// A server-to-client request, in whichever direction this revision sends it.
pub(super) enum Ask {
    /// `elicitation/create`.
    Elicit(ElicitRequestParams),
    /// `sampling/createMessage`.
    Sample(CreateMessageRequestParams),
}

impl Ask {
    /// This ask as an MRTR `inputRequests` entry.
    fn into_input_request(self) -> InputRequest {
        match self {
            Self::Elicit(params) => InputRequest::Elicitation(ElicitRequest::new(params)),
            Self::Sample(params) => InputRequest::CreateMessage(CreateMessageRequest::new(params)),
        }
    }
}

/// Obtains `ask`'s answer by the mechanism `revision` defines.
///
/// `state` and `responses` are the previous round's, as the client re-sent
/// them; both are `None` on a first call.
pub(super) async fn ask(
    context: &RequestContext<RoleServer>,
    revision: ServedRevision,
    round: Round,
    ask: Ask,
) -> Result<Interaction, ErrorData> {
    if !revision.is_stateless() {
        return legacy(context, ask).await.map(Interaction::Answered);
    }
    if let Some(answer) = round.answer() {
        return Ok(Interaction::Answered(answer));
    }
    // Either a first call, or MRTR-024: the client retried without the
    // information the server asked for, so the server asks again rather than
    // failing a call the client can still complete.
    let mut requests = InputRequests::new();
    requests.insert(KEY.to_owned(), ask.into_input_request());
    Ok(Interaction::Deferred(InputRequiredResult::new(
        Some(requests),
        Some(round.next_state()),
    )))
}

/// What a `tools/call` carried from the previous MRTR round.
pub(super) struct Round {
    /// The tool being called, which names the state this server issues.
    pub(super) tool: &'static str,
    /// The `requestState` the client echoed back, if any.
    pub(super) state: Option<String>,
    /// The `inputResponses` the client supplied, if any.
    pub(super) responses: Option<rmcp::model::InputResponses>,
}

impl Round {
    /// The answer this round carries, if the client supplied one.
    fn answer(&self) -> Option<Value> {
        self.responses.as_ref()?.get(KEY).cloned()
    }

    /// The `requestState` to issue with this round's `InputRequiredResult`.
    ///
    /// It carries the tool's name and the round number, and **no authority**:
    /// MRTR-007 says a server must treat `requestState` as attacker-controlled,
    /// and MRTR-008's integrity obligation is conditioned on the state
    /// influencing "authorization, resource access, or business logic". This
    /// one influences none — the retry re-sends the arguments, and the server
    /// re-derives everything from them — so there is nothing an attacker gains
    /// by forging it, and signing it would imply a trust this value does not
    /// carry.
    fn next_state(&self) -> String {
        let previous = self
            .state
            .as_deref()
            .and_then(|state| state.rsplit_once(':'))
            .and_then(|(_, round)| round.parse::<u32>().ok())
            .unwrap_or(0);
        format!("{}:{}", self.tool, previous.saturating_add(1))
    }
}

/// The `2025-11-25` flow: send the request and await the client's answer.
async fn legacy(context: &RequestContext<RoleServer>, ask: Ask) -> Result<Value, ErrorData> {
    let (result, what) = match ask {
        Ask::Elicit(params) => (
            context
                .peer
                .create_elicitation(params)
                .await
                .map(serde_json::to_value),
            "elicitation/create",
        ),
        Ask::Sample(params) => (
            context
                .peer
                .create_message(params)
                .await
                .map(serde_json::to_value),
            "sampling/createMessage",
        ),
    };
    let value = result.map_err(|error| {
        ErrorData::internal_error(
            format!("{what} failed"),
            Some(serde_json::json!({ "error": error.to_string() })),
        )
    })?;
    value.map_err(|error| {
        ErrorData::internal_error(
            format!("{what} result is not representable"),
            Some(serde_json::json!({ "error": error.to_string() })),
        )
    })
}
