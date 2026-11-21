// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use serde_json::json;

use super::{identity, read_only};

#[test]
fn a_tool_is_read_only_only_when_the_server_says_so_and_does_not_also_say_destructive() {
    assert!(read_only(
        &json!({"name": "r", "annotations": {"readOnlyHint": true}})
    ));
    assert!(read_only(
        &json!({"annotations": {"readOnlyHint": true, "destructiveHint": false}})
    ));
    for tool in [
        json!({"name": "unannotated"}),
        json!({"annotations": {}}),
        json!({"annotations": {"readOnlyHint": false}}),
        json!({"annotations": {"readOnlyHint": "true"}}),
        json!({"annotations": {"readOnlyHint": true, "destructiveHint": true}}),
        json!({"readOnlyHint": true}),
    ] {
        assert!(!read_only(&tool), "{tool}");
    }
}

#[test]
fn a_server_is_named_by_its_own_report_or_not_at_all() {
    assert_eq!(
        identity(Some(&json!({"name": "s", "version": "1.2"}))).as_deref(),
        Some("s 1.2")
    );
    assert_eq!(identity(Some(&json!({"name": "s"}))).as_deref(), Some("s"));
    assert_eq!(identity(Some(&json!({"version": "1.2"}))), None);
    assert_eq!(identity(None), None);
}
