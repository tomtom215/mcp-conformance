// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Driving one `subscriptions/listen` stream to its end.
//!
//! `2026-07-28` replaced `resources/subscribe` with a long-lived request: the
//! client names the notification categories it wants, the server acknowledges
//! the subset it will send, notifications arrive tagged with the
//! subscription's id, and the stream ends — gracefully, with an empty result,
//! when the server tears it down.
//!
//! rmcp's client owns the protocol here, including the parts a host would
//! otherwise get wrong: it rejects an acknowledgment that arrives after a
//! notification (SUBS-002 from the client's side), and rejects one
//! acknowledging a filter the client never requested. What this module adds is
//! the *reason to run it* — a bounded drain that ends on its own, so a
//! recording of a subscription is a finite artifact rather than a process that
//! has to be killed.

use rmcp::model::{ServerNotification, SubscriptionFilter};
use rmcp::service::{RoleClient, RunningService, Service, SubscriptionEnd};

#[cfg(test)]
mod tests;

/// What one subscription produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SubscriptionReport {
    /// The categories the server acknowledged, as it wrote them.
    pub acknowledged: Vec<String>,
    /// The notification methods that arrived, in order.
    pub notifications: Vec<String>,
    /// How the stream ended, named the way the operator would say it.
    pub ended: String,
}

/// The filter a recording asks for.
///
/// Every category the revision defines, plus one resource URI this server has
/// and one it does not. The unknown URI is the point of including it: a server
/// may only acknowledge what it can actually serve, so the acknowledgment in
/// the recording shows the narrowing rather than leaving it to be assumed.
#[must_use]
pub fn everything(known_uri: &str, unknown_uri: &str) -> SubscriptionFilter {
    let mut filter = SubscriptionFilter::new();
    filter.tools_list_changed = Some(true);
    filter.prompts_list_changed = Some(true);
    filter.resources_list_changed = Some(true);
    filter.resource_subscriptions = Some(vec![known_uri.to_owned(), unknown_uri.to_owned()]);
    filter
}

/// Opens a subscription with `filter` and drains it until the stream ends.
///
/// # Errors
///
/// Returns the transport or protocol error that prevented the subscription
/// from opening, or that ended it abnormally.
pub async fn drain<S: Service<RoleClient>>(
    client: &RunningService<RoleClient, S>,
    filter: SubscriptionFilter,
) -> Result<SubscriptionReport, rmcp::service::ServiceError> {
    let mut subscription = client.peer().listen(filter).await?;
    let mut report = SubscriptionReport {
        acknowledged: categories(subscription.acknowledged()),
        ..SubscriptionReport::default()
    };
    // `next` returns `None` at the end of the stream, whichever way it ended;
    // there is no timeout here on purpose, because a stream that never ends is
    // a server defect this host should surface as a hang rather than hide as a
    // truncated recording.
    while let Some(notification) = subscription.next().await? {
        report.notifications.push(method_of(&notification));
    }
    report.ended = match subscription.end() {
        Some(SubscriptionEnd::Graceful(_)) => "graceful".to_owned(),
        Some(SubscriptionEnd::Cancelled) => "cancelled".to_owned(),
        Some(SubscriptionEnd::Abrupt) => "abrupt".to_owned(),
        Some(other) => format!("{other:?}"),
        None => "still open".to_owned(),
    };
    Ok(report)
}

/// The categories `filter` carries, as wire names.
fn categories(filter: &SubscriptionFilter) -> Vec<String> {
    let mut categories = Vec::new();
    for (asked, name) in [
        (filter.tools_list_changed, "toolsListChanged"),
        (filter.prompts_list_changed, "promptsListChanged"),
        (filter.resources_list_changed, "resourcesListChanged"),
    ] {
        if asked == Some(true) {
            categories.push(name.to_owned());
        }
    }
    categories.extend(
        filter
            .resource_subscriptions
            .iter()
            .flatten()
            .map(|uri| format!("resource:{uri}")),
    );
    categories
}

/// A notification's method, read from its wire form.
///
/// Serialized rather than matched: `ServerNotification` is `#[non_exhaustive]`
/// and grows with the protocol, and a match would answer "unknown" for exactly
/// the notification a recording most needs named.
fn method_of(notification: &ServerNotification) -> String {
    serde_json::to_value(notification)
        .ok()
        .and_then(|wire| wire.get("method")?.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "(unnamed)".to_owned())
}
