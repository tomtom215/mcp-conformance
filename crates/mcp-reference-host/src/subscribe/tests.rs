// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the pure parts: the filter a recording asks for, and how a
//! subscription's contents are named.
//!
//! [`drain`] itself needs a live server, and gets one in
//! `tests/subscriptions.rs`, which drives the whole lifecycle against the real
//! stateless server: the server narrowing the filter to what it can serve,
//! every acknowledged category arriving, and the stream ending on the server's
//! own initiative rather than by the client giving up.

use super::*;

#[test]
fn the_recording_filter_asks_for_every_category() {
    // A filter that omitted one would leave that category's acknowledgment and
    // its notification out of every capture, which is the same as not having
    // implemented it as far as the corpus is concerned.
    let filter = everything("test://known", "test://unknown");
    assert_eq!(filter.tools_list_changed, Some(true));
    assert_eq!(filter.prompts_list_changed, Some(true));
    assert_eq!(filter.resources_list_changed, Some(true));
    assert_eq!(
        filter.resource_subscriptions,
        Some(vec!["test://known".to_owned(), "test://unknown".to_owned()]),
        "both, so the acknowledgment shows the server narrowing"
    );
}

#[test]
fn categories_names_only_what_was_asked_for() {
    // The report is what an operator reads, and a category listed because the
    // field was present rather than true would misreport the acknowledgment.
    let mut filter = SubscriptionFilter::new();
    filter.tools_list_changed = Some(true);
    filter.prompts_list_changed = Some(false);
    filter.resources_list_changed = None;
    assert_eq!(categories(&filter), ["toolsListChanged"]);
}

#[test]
fn categories_names_each_subscribed_resource() {
    let mut filter = SubscriptionFilter::new();
    filter.resource_subscriptions = Some(vec!["test://a".to_owned(), "test://b".to_owned()]);
    assert_eq!(
        categories(&filter),
        ["resource:test://a", "resource:test://b"]
    );
}

#[test]
fn an_empty_filter_names_nothing() {
    assert!(categories(&SubscriptionFilter::new()).is_empty());
}

#[test]
fn a_notification_is_named_by_its_wire_method() {
    // Read from the serialized form because `ServerNotification` is
    // `#[non_exhaustive]`: a match arm per variant would report "(unnamed)"
    // for whichever notification the protocol adds next, which is exactly the
    // one a recording would need named.
    assert_eq!(
        method_of(&ServerNotification::ToolListChangedNotification(
            rmcp::model::ToolListChangedNotification::default()
        )),
        "notifications/tools/list_changed"
    );
    assert_eq!(
        method_of(&ServerNotification::ResourceUpdatedNotification(
            rmcp::model::ResourceUpdatedNotification::new(
                rmcp::model::ResourceUpdatedNotificationParam::new("test://a")
            )
        )),
        "notifications/resources/updated"
    );
}
