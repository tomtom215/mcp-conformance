// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the two decisions this module owns.
//!
//! [`run`] needs a live `SubscriptionContext`, which only rmcp can build, so
//! the announcement *list* is tested here and the announcement *stream* is
//! tested on the wire by `tests/stateless_stdio.rs`.

use super::*;

/// The method each notification names on the wire.
fn methods(accepted: &SubscriptionFilter) -> Vec<String> {
    announcements(accepted)
        .iter()
        .map(|notification| {
            serde_json::to_value(notification).expect("serializes")["method"]
                .as_str()
                .expect("a method")
                .to_owned()
        })
        .collect()
}

/// A filter requesting every category, with `uris` as its resources.
fn everything(uris: &[&str]) -> SubscriptionFilter {
    let mut filter = SubscriptionFilter::new();
    filter.tools_list_changed = Some(true);
    filter.prompts_list_changed = Some(true);
    filter.resources_list_changed = Some(true);
    filter.resource_subscriptions = Some(uris.iter().map(|uri| (*uri).to_owned()).collect());
    filter
}

#[test]
fn the_legacy_revision_leaves_the_method_unimplemented() {
    // `subscriptions/listen` does not exist at `2025-11-25`, where
    // `resources/subscribe` is the mechanism. Advertising it there would offer
    // a stream rmcp's own dispatch refuses to open.
    assert_eq!(
        accepted(ServedRevision::V2025_11_25, &everything(&[])),
        None
    );
}

#[test]
fn a_resource_this_server_does_not_have_is_not_acknowledged() {
    // The narrowing this module exists for: acknowledging an unknown URI is
    // promising updates for a resource the server has never heard of.
    let accepted = accepted(
        ServedRevision::V2026_07_28,
        &everything(&["file:///nowhere", resources::STATIC_TEXT_URI]),
    )
    .expect("the stateless revision implements it");
    assert_eq!(
        accepted.resource_subscriptions,
        Some(vec![resources::STATIC_TEXT_URI.to_owned()])
    );
}

#[test]
fn no_known_resource_leaves_the_category_absent_rather_than_empty() {
    // `Some([])` and `None` mean different things to a client reading the
    // acknowledgment: an empty list says "I accepted resource subscriptions,
    // just none of yours", which is not what happened.
    let accepted = accepted(
        ServedRevision::V2026_07_28,
        &everything(&["file:///nowhere"]),
    )
    .expect("implemented");
    assert_eq!(accepted.resource_subscriptions, None);
}

#[test]
fn a_category_not_requested_is_not_accepted() {
    // SUBS-001 is enforced by rmcp's sink, but the acknowledgment is what the
    // client checks (SUBS-003), and it must not claim more than was asked.
    let mut requested = SubscriptionFilter::new();
    requested.tools_list_changed = Some(true);
    let accepted = accepted(ServedRevision::V2026_07_28, &requested).expect("implemented");
    assert_eq!(accepted.tools_list_changed, Some(true));
    assert_eq!(accepted.prompts_list_changed, None);
    assert_eq!(accepted.resources_list_changed, None);
    assert_eq!(accepted.resource_subscriptions, None);
}

#[test]
fn a_category_requested_as_false_is_carried_through_as_false() {
    // "Omitting a field is equivalent to not subscribing", and so is `false`.
    // Rewriting it to `None` would be tidier and would also change what the
    // acknowledgment says the client asked for.
    let mut requested = SubscriptionFilter::new();
    requested.tools_list_changed = Some(false);
    let accepted = accepted(ServedRevision::V2026_07_28, &requested).expect("implemented");
    assert_eq!(accepted.tools_list_changed, Some(false));
    assert!(
        methods(&accepted).is_empty(),
        "and nothing is announced for it"
    );
}

#[test]
fn every_accepted_category_is_announced_once_in_a_fixed_order() {
    // Fixed order because a recording of this server is a corpus fixture: a
    // per-run ordering would churn the golden report with nothing changed.
    let accepted = accepted(
        ServedRevision::V2026_07_28,
        &everything(&[resources::STATIC_TEXT_URI, resources::STATIC_BINARY_URI]),
    )
    .expect("implemented");
    assert_eq!(
        methods(&accepted),
        [
            "notifications/tools/list_changed",
            "notifications/prompts/list_changed",
            "notifications/resources/list_changed",
            "notifications/resources/updated",
            "notifications/resources/updated",
        ]
    );
}

#[test]
fn an_empty_filter_announces_nothing() {
    // A subscription that asked for nothing gets nothing — not a "here is
    // everything" default, which is the exact shape SUBS-001 forbids.
    let accepted =
        accepted(ServedRevision::V2026_07_28, &SubscriptionFilter::new()).expect("implemented");
    assert!(methods(&accepted).is_empty());
}

#[test]
fn a_resource_update_names_the_resource_it_is_about() {
    let mut requested = SubscriptionFilter::new();
    requested.resource_subscriptions = Some(vec![resources::STATIC_TEXT_URI.to_owned()]);
    let accepted = accepted(ServedRevision::V2026_07_28, &requested).expect("implemented");
    let announced = announcements(&accepted);
    let wire = serde_json::to_value(&announced[0]).expect("serializes");
    assert_eq!(wire["params"]["uri"], resources::STATIC_TEXT_URI, "{wire}");
}
