// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One cancellation, and one call after it.
//!
//! `2026-07-28` binds both ends of cancellation: a `notifications/cancelled`
//! must name the request it cancels (`TRAN-123`), and a server must send
//! nothing further *for that request* once it has (`TRAN-124`, `TRAN-070`).
//! Neither has anything to judge in a session that never cancels anything, so
//! both report *not observed* on every recording this corpus holds.
//!
//! **A `MUST NOT` cannot be witnessed by an absence.** The forbidden message is
//! the one that is not there, and a recording of nothing proves nothing — so
//! what this drives is a cancellation followed by a *permitted* server message,
//! which is the only shape a trace can carry for the clause. The permitted
//! message is an ordinary tool call: the server answers it, the check examines
//! that answer while a cancellation stands, and finds it belongs to a different
//! request.
//!
//! **The cancelled request is one the server has already answered**, and that
//! is a deliberate choice rather than a convenience. A recording committed as a
//! byte-pinned golden cannot contain a race, and cancelling an outstanding
//! request is one: whether the answer beats the notification depends on
//! scheduling, so the same session would record two different traces on two
//! runs. A late cancel — the user pressing stop as the answer lands — is real
//! client behaviour, it names a real request, and it puts the server under
//! exactly the obligation the clause states. What it cannot show is a server
//! *abandoning work*, which no trace-level check reads anyway.

use rmcp::model::{CallToolRequestParams, CancelledNotificationParam, ClientRequest};
use rmcp::service::{Peer, PeerRequestOptions, RoleClient};

/// The reason the recording gives, so the notification is readable rather than
/// bare.
const REASON: &str = "recording a cancellation the server must honour";

/// What the cancellation round drove, as observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelReport {
    /// The tool whose call was cancelled, and the request id it carried.
    pub cancelled: String,
    /// The call made afterwards, whose answer the recording needs.
    pub after: Result<String, String>,
}

/// Cancels one answered `tool` call, then calls `tool` again.
///
/// # Errors
///
/// Returns the transport or protocol error that stopped the round; the caller
/// decides whether that ends the run.
pub async fn round(peer: &Peer<RoleClient>, tool: &str) -> Result<CancelReport, String> {
    let handle = peer
        .send_cancellable_request(
            ClientRequest::CallToolRequest(rmcp::model::Request::new(CallToolRequestParams::new(
                tool.to_owned(),
            ))),
            PeerRequestOptions::no_options(),
        )
        .await
        .map_err(|error| format!("cancellable {tool} call could not be sent: {error}"))?;
    let id = handle.id.clone();
    // Awaited before cancelling, so the notification lands after the answer and
    // the recording is the same on every run.
    handle
        .await_response()
        .await
        .map_err(|error| format!("cancellable {tool} call failed: {error}"))?;
    peer.notify_cancelled(CancelledNotificationParam::new(
        Some(id.clone()),
        Some(REASON.to_owned()),
    ))
    .await
    .map_err(|error| format!("cancellation for {id} could not be sent: {error}"))?;

    // The message the clause is actually about: something the server *may*
    // send while a cancellation stands.
    let after = peer
        .call_tool(CallToolRequestParams::new(tool.to_owned()))
        .await
        .map(|result| format!("{} content item(s)", result.content.len()))
        .map_err(|error| error.to_string());
    Ok(CancelReport {
        cancelled: format!("{tool} (request {id})"),
        after,
    })
}
