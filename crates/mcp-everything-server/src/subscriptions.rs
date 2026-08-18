// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `subscriptions/listen` — the stream that replaced `resources/subscribe`.
//!
//! `2026-07-28` folds every "tell me when this changes" mechanism into one
//! long-lived request. The client names the notification categories it wants,
//! the server acknowledges with the subset it will actually send, and every
//! notification on the stream carries the subscription's id in `_meta` so a
//! client on stdio — where one channel carries everything — can tell which
//! subscription a message belongs to.
//!
//! Most of the specification's obligations are rmcp's to keep, and it keeps
//! them: the acknowledgment goes out before this module's [`run`] is called
//! (SUBS-002), rmcp's `SubscriptionSink::send` refuses any category the accepted
//! filter does not carry (SUBS-001), and returning `Ok(())` makes the SDK emit
//! the empty final result the graceful-closure clauses ask for (SUBS-005,
//! SUBS-006). What is left here is the two decisions no SDK can make for a
//! server: **which categories it can honestly serve**, and **what it does with
//! the stream once it is open**.
//!
//! The `2025-11-25` surface is untouched. rmcp answers `subscriptions/listen`
//! with `method not found` for a legacy request before consulting any of this,
//! and [`accepted`] returns `None` for that revision anyway — a server that
//! advertised the newer mechanism while serving the older one would be
//! offering a stream its own transport refuses to open.

use rmcp::model::{
    ResourceUpdatedNotificationParam, ServerNotification, SubscriptionFilter,
    ToolListChangedNotification,
};
use rmcp::service::SubscriptionContext;
use rmcp::{ErrorData, model::PromptListChangedNotification};

use crate::resources;
use crate::server::ServedRevision;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

/// The categories this server can produce, narrowed to what `requested` asks
/// for; `None` leaves `subscriptions/listen` unimplemented.
///
/// rmcp intersects whatever comes back with both the request and the
/// capabilities [`get_info`](crate::EverythingServer) advertises, so this
/// cannot widen anything — its job is to *narrow*, and it narrows in the one
/// place the SDK cannot: resource URIs. A server that acknowledged a
/// subscription to `file:///anything` would be promising updates for a
/// resource it has never heard of, and SUBS-003 exists precisely so a client
/// can notice the difference between what it asked for and what it got.
#[must_use]
pub fn accepted(
    revision: ServedRevision,
    requested: &SubscriptionFilter,
) -> Option<SubscriptionFilter> {
    if !revision.is_stateless() {
        return None;
    }
    let known: Vec<String> = requested
        .resource_subscriptions
        .iter()
        .flatten()
        .filter(|uri| resources::read(uri).is_some())
        .cloned()
        .collect();
    let mut accepted = SubscriptionFilter::new();
    accepted.tools_list_changed = requested.tools_list_changed;
    accepted.prompts_list_changed = requested.prompts_list_changed;
    accepted.resources_list_changed = requested.resources_list_changed;
    accepted.resource_subscriptions = (!known.is_empty()).then_some(known);
    Some(accepted)
}

/// Runs one subscription: announce everything the filter covers, then end it.
///
/// The announcement is the idiom this crate's `2025-11-25`
/// `resources/subscribe` already uses — one notification per accepted
/// category, so a client (and a recording) can see the stream work rather than
/// infer it from silence. The specification leaves update timing to the
/// server, and a subscription that never says anything is indistinguishable
/// from one that is broken.
///
/// **Then it ends, on the server's initiative**, which is what makes the SDK
/// emit the empty final result of SUBS-005 and SUBS-006. Two things make that
/// the honest choice here rather than a convenient one:
///
/// - *This* server has no ongoing source of change. Its tool, prompt and
///   resource catalogues are compile-time constants; once the announcements
///   are out there is nothing further it could ever send. Holding the stream
///   open would be a promise of updates that cannot arrive.
/// - Ending is the only way any recording can exhibit graceful closure at all.
///   A client-side cancellation does not produce the final result — rmcp
///   suppresses the response for a cancelled request, verified on the wire —
///   so a stream that ends only when the client says so leaves both closure
///   clauses permanently unexercised.
///
/// A server with real change to report would hold the stream open and close it
/// at shutdown instead, which is the case the clauses are written around. That
/// is a difference in what the server *has to say*, not in what it owes the
/// protocol, and SUBS-007 already tells a client to re-establish after a close.
///
/// # Errors
///
/// Never returns `Err`: a failed send means the client is gone, which ends the
/// subscription rather than failing the request.
pub async fn run(context: SubscriptionContext) -> Result<(), ErrorData> {
    for notification in announcements(context.accepted()) {
        // Send failures are the subscription ending underneath us — a closed
        // stream, or a filter this server got wrong. Neither is worth failing
        // the listen request over, and the sink has already refused anything
        // the filter does not carry.
        if context.sink().send(notification).await.is_err() {
            return Ok(());
        }
    }
    Ok(())
}

/// One notification per category in `accepted`, in a fixed order.
///
/// Ordered, not arbitrary: a recording of this server is a corpus fixture, and
/// a set iteration order that varied per run would make the golden report
/// churn without anything having changed.
fn announcements(accepted: &SubscriptionFilter) -> Vec<ServerNotification> {
    let mut announcements = Vec::new();
    if accepted.tools_list_changed == Some(true) {
        announcements.push(ServerNotification::ToolListChangedNotification(
            ToolListChangedNotification::default(),
        ));
    }
    if accepted.prompts_list_changed == Some(true) {
        announcements.push(ServerNotification::PromptListChangedNotification(
            PromptListChangedNotification::default(),
        ));
    }
    if accepted.resources_list_changed == Some(true) {
        announcements.push(ServerNotification::ResourceListChangedNotification(
            rmcp::model::ResourceListChangedNotification::default(),
        ));
    }
    for uri in accepted.resource_subscriptions.iter().flatten() {
        announcements.push(ServerNotification::ResourceUpdatedNotification(
            rmcp::model::ResourceUpdatedNotification::new(ResourceUpdatedNotificationParam::new(
                uri.clone(),
            )),
        ));
    }
    announcements
}
