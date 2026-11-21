// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use serde_json::json;

use super::{Failure, Incoming, classify};

#[test]
fn only_the_answer_to_the_awaited_id_ends_an_exchange() {
    assert!(matches!(
        classify(json!({"jsonrpc": "2.0", "id": 4, "result": {}}), Some(4)),
        Incoming::Answer(_)
    ));
    assert!(matches!(
        classify(
            json!({"jsonrpc": "2.0", "id": 4, "error": {"code": -1, "message": "m"}}),
            Some(4)
        ),
        Incoming::Answer(_)
    ));
    for other in [
        json!({"jsonrpc": "2.0", "id": 5, "result": {}}),
        json!({"jsonrpc": "2.0", "id": "4", "result": {}}),
        json!({"jsonrpc": "2.0", "id": null, "error": {"code": -1, "message": "m"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/progress"}),
        json!({"jsonrpc": "2.0", "id": 4}),
        json!("not a message"),
    ] {
        assert!(
            matches!(classify(other.clone(), Some(4)), Incoming::Other),
            "{other}"
        );
    }
    // Nothing answers a notification.
    assert!(matches!(
        classify(json!({"jsonrpc": "2.0", "id": 4, "result": {}}), None),
        Incoming::Other
    ));
}

#[test]
fn a_message_with_a_method_and_an_id_is_the_servers_request_whatever_its_id() {
    assert!(matches!(
        classify(
            json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}),
            Some(4)
        ),
        Incoming::Request(_)
    ));
    assert!(matches!(
        classify(
            json!({"jsonrpc": "2.0", "id": "s-1", "method": "roots/list"}),
            Some(4)
        ),
        Incoming::Request(_)
    ));
}

#[test]
fn each_failure_says_what_happened_and_which_flag_bounds_it() {
    assert!(Failure::Timeout.to_string().contains("--timeout"));
    assert!(
        Failure::TooLarge
            .to_string()
            .contains("--max-response-bytes")
    );
    assert!(Failure::Http(404).to_string().contains("HTTP 404"));
    assert!(Failure::Closed.to_string().contains("ended the session"));
    assert_eq!(
        Failure::Transport("as given".to_owned()).to_string(),
        "as given"
    );
}
