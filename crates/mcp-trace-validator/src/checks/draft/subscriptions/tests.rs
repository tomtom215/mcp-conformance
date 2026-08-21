// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `subscriptions/listen` clauses.
//!
//! The interleaving case is the one worth pinning: on stdio every subscription
//! shares a channel, so "first message" is per subscription id and a second
//! subscription's traffic ahead of this one's acknowledgment must not be read as
//! one. A check that compared raw event order would pass every test but that one.

use crate::checks::draft::testkit::{client, findings_for, server, trace};

const ONLY_REQUESTED: &str = "subscriptions.only-requested-notifications";
const ACK_FIRST: &str = "subscriptions.acknowledgment-first";
const CLOSE_SHAPE: &str = "subscriptions.graceful-close-result-shape";

/// A `subscriptions/listen` request `id` whose filter is `filter`.
fn listen(seq: u64, id: u64, filter: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"subscriptions/listen","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{{}}}},"notifications":{filter}}}}}"#
        ),
    )
}

/// A server notification on subscription `id`, with `extra` params.
fn notify(seq: u64, id: u64, method: &str, extra: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"{method}","params":{{"_meta":{{"io.modelcontextprotocol/subscriptionId":{id}}}{extra}}}}}"#
        ),
    )
}

/// The acknowledgment for subscription `id`.
fn ack(seq: u64, id: u64) -> String {
    notify(seq, id, "notifications/subscriptions/acknowledged", "")
}

/// The `subscriptions/listen` response closing subscription `id`, with `extra`.
fn close(seq: u64, id: u64, extra: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete","_meta":{{"io.modelcontextprotocol/subscriptionId":{id}}}{extra}}}}}"#
        ),
    )
}

#[test]
fn a_requested_notification_type_is_delivered_without_complaint() {
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        notify(2, 1, "notifications/tools/list_changed", ""),
    ]);
    assert!(findings_for(ONLY_REQUESTED, &session).is_empty());
    assert!(findings_for(ACK_FIRST, &session).is_empty());
}

#[test]
fn an_unrequested_type_is_reported() {
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        notify(2, 1, "notifications/prompts/list_changed", ""),
    ]);
    let findings = findings_for(ONLY_REQUESTED, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("promptsListChanged"), "{findings:?}");
}

#[test]
fn a_filter_field_set_false_is_not_a_request() {
    // "Omitting a field is equivalent to not subscribing"; setting it to false is
    // no stronger a request than omitting it.
    for filter in [r#"{"toolsListChanged":false}"#, "{}"] {
        let session = trace(&[
            listen(0, 1, filter),
            ack(1, 1),
            notify(2, 1, "notifications/tools/list_changed", ""),
        ]);
        assert_eq!(
            findings_for(ONLY_REQUESTED, &session).len(),
            1,
            "filter {filter}"
        );
    }
}

#[test]
fn resource_updates_are_matched_by_uri() {
    let subscribed = r#"{"resourceSubscriptions":["file:///a.json"]}"#;
    let wanted = trace(&[
        listen(0, 1, subscribed),
        ack(1, 1),
        notify(
            2,
            1,
            "notifications/resources/updated",
            r#","uri":"file:///a.json""#,
        ),
    ]);
    assert!(findings_for(ONLY_REQUESTED, &wanted).is_empty());

    let unwanted = trace(&[
        listen(0, 1, subscribed),
        ack(1, 1),
        notify(
            2,
            1,
            "notifications/resources/updated",
            r#","uri":"file:///b.json""#,
        ),
    ]);
    let findings = findings_for(ONLY_REQUESTED, &unwanted);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("file:///b.json"), "{findings:?}");
}

#[test]
fn a_type_outside_the_filter_vocabulary_is_reported() {
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        notify(2, 1, "notifications/message", ""),
    ]);
    assert_eq!(findings_for(ONLY_REQUESTED, &session).len(), 1);
}

#[test]
fn an_untagged_notification_belongs_to_no_subscription() {
    // Progress and log notifications relate to an in-flight request, not to a
    // subscription, and carry no subscription id. Judging them here would report
    // every ordinary session.
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        server(
            2,
            r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"x"}}"#,
        ),
    ]);
    assert!(findings_for(ONLY_REQUESTED, &session).is_empty());
}

#[test]
fn the_acknowledgment_must_come_first_on_its_own_subscription() {
    let jumped = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        notify(1, 1, "notifications/tools/list_changed", ""),
        ack(2, 1),
    ]);
    let findings = findings_for(ACK_FIRST, &jumped);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("list_changed"), "{findings:?}");
}

#[test]
fn another_subscriptions_traffic_may_interleave_ahead_of_the_acknowledgment() {
    // The stdio carve-out, stated on the page: ordering is per subscription id,
    // not per channel.
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        listen(1, 2, r#"{"promptsListChanged":true}"#),
        ack(2, 1),
        notify(3, 1, "notifications/tools/list_changed", ""),
        ack(4, 2),
        notify(5, 2, "notifications/prompts/list_changed", ""),
    ]);
    assert!(findings_for(ACK_FIRST, &session).is_empty());
    assert!(findings_for(ONLY_REQUESTED, &session).is_empty());
}

#[test]
fn a_second_subscriptions_notification_does_not_open_this_one() {
    // The first tagged message per *id*, not the first message that happens to
    // follow the listen request: subscription 1's ordinary notification arrives
    // between subscription 2's request and its acknowledgment, and neither is at
    // fault.
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        listen(2, 2, r#"{"promptsListChanged":true}"#),
        notify(3, 1, "notifications/tools/list_changed", ""),
        ack(4, 2),
        notify(5, 2, "notifications/prompts/list_changed", ""),
    ]);
    assert!(findings_for(ACK_FIRST, &session).is_empty());
    assert!(findings_for(ONLY_REQUESTED, &session).is_empty());
}

#[test]
fn a_subscription_with_nothing_on_it_yet_is_not_reported() {
    // A recording that ends before the acknowledgment arrives is not evidence
    // that it never did.
    let session = trace(&[listen(0, 1, r#"{"toolsListChanged":true}"#)]);
    assert!(findings_for(ACK_FIRST, &session).is_empty());
}

#[test]
fn closing_before_acknowledging_is_reported() {
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        close(1, 1, ""),
    ]);
    let findings = findings_for(ACK_FIRST, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("closed"), "{findings:?}");
}

#[test]
fn a_graceful_closure_carries_an_empty_result() {
    let empty = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        close(2, 1, ""),
    ]);
    assert!(findings_for(CLOSE_SHAPE, &empty).is_empty());

    let stuffed = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        close(2, 1, r#","delivered":7"#),
    ]);
    let findings = findings_for(CLOSE_SHAPE, &stuffed);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("delivered"), "{findings:?}");
}

#[test]
fn a_stream_that_simply_stops_is_not_a_missing_response() {
    // The page calls a close without a response an unexpected disconnect rather
    // than a broken promise, so an unanswered `subscriptions/listen` reports
    // nothing here.
    let session = trace(&[
        listen(0, 1, r#"{"toolsListChanged":true}"#),
        ack(1, 1),
        notify(2, 1, "notifications/tools/list_changed", ""),
    ]);
    assert!(findings_for(CLOSE_SHAPE, &session).is_empty());
}
