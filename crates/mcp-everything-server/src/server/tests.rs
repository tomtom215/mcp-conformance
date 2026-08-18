// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Unit tests for [`super::EverythingServer`]: what it advertises, what it
//! tracks, and which revision it presents.
//!
//! The wire-level counterparts live in `tests/http_policy.rs` (the legacy
//! surface) and `tests/stateless_http.rs` (the `2026-07-28` one) — a handler
//! test cannot see a transport that rejects a request before dispatch, and a
//! transport test cannot see a field the serializer skips.

use super::*;

#[test]
fn advertises_exactly_the_implemented_capabilities() {
    let info = EverythingServer::new().get_info();
    let capabilities = info.capabilities;
    assert!(capabilities.tools.is_some(), "tools are implemented");
    assert!(
        capabilities.resources.is_some(),
        "resources are implemented"
    );
    assert_eq!(
        capabilities.resources.as_ref().unwrap().subscribe,
        Some(true),
        "subscriptions are implemented"
    );
    assert!(capabilities.prompts.is_some(), "prompts are implemented");
    for (declared, name) in [
        (capabilities.tools.as_ref().unwrap().list_changed, "tools"),
        (
            capabilities.resources.as_ref().unwrap().list_changed,
            "resources",
        ),
        (
            capabilities.prompts.as_ref().unwrap().list_changed,
            "prompts",
        ),
    ] {
        assert_eq!(
            declared,
            Some(true),
            "{name} listChanged is implemented via the test-list-changed tool"
        );
    }
    assert!(capabilities.logging.is_some(), "logging is implemented");
    assert!(
        capabilities.completions.is_some(),
        "completions are implemented"
    );
}

#[test]
fn pins_the_protocol_revision_the_registry_covers() {
    let info = EverythingServer::new().get_info();
    assert_eq!(info.protocol_version, ProtocolVersion::V_2025_11_25);
}

#[test]
fn names_itself_from_the_crate_metadata() {
    let info = EverythingServer::new().get_info();
    assert_eq!(info.server_info.name, env!("CARGO_PKG_NAME"));
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn subscription_tracking_inserts_removes_and_reports_sorted() {
    let server = EverythingServer::new();
    assert!(server.track_subscription("test://b".into()), "new URI");
    assert!(server.track_subscription("test://a".into()), "second URI");
    assert!(
        !server.track_subscription("test://a".into()),
        "duplicate is not newly tracked"
    );
    assert_eq!(server.subscribed_uris(), ["test://a", "test://b"]);
    assert!(server.untrack_subscription("test://a"), "tracked URI drops");
    assert!(
        !server.untrack_subscription("test://a"),
        "second drop is a no-op"
    );
    assert_eq!(server.subscribed_uris(), ["test://b"]);
}

#[test]
fn subscription_set_is_capped_against_unbounded_growth() {
    let server = EverythingServer::new();
    for index in 0..MAX_SUBSCRIPTIONS {
        assert!(
            server.track_subscription(format!("test://uri/{index}")),
            "URI {index} within the cap is tracked"
        );
    }
    // At the cap, a new distinct URI is refused — the set cannot grow.
    assert!(
        !server.track_subscription("test://one-too-many".into()),
        "a new URI past the cap must be refused"
    );
    assert_eq!(server.subscribed_uris().len(), MAX_SUBSCRIPTIONS);
    // An already-tracked URI at the cap still reports "not newly tracked"
    // (idempotent), never an error.
    assert!(!server.track_subscription("test://uri/0".into()));
    assert_eq!(server.subscribed_uris().len(), MAX_SUBSCRIPTIONS);
    // Dropping one frees a slot again.
    assert!(server.untrack_subscription("test://uri/0"));
    assert!(server.track_subscription("test://after-eviction".into()));
}

#[test]
fn log_threshold_starts_permissive_and_tightens() {
    let server = EverythingServer::new();
    assert!(server.log_permitted_for(None, LoggingLevel::Debug));
    *server.log_level.lock().unwrap() = LoggingLevel::Error;
    assert!(!server.log_permitted_for(None, LoggingLevel::Info));
    assert!(server.log_permitted_for(None, LoggingLevel::Critical));
}

#[test]
fn the_served_revision_chooses_what_get_info_negotiates() {
    assert_eq!(
        EverythingServer::serving(ServedRevision::V2026_07_28)
            .get_info()
            .protocol_version,
        ProtocolVersion::V_2026_07_28
    );
    assert_eq!(
        EverythingServer::new().revision(),
        ServedRevision::V2025_11_25,
        "the default constructor must keep serving the pinned revision"
    );
}

#[test]
fn only_the_stateless_mode_narrows_the_advertised_versions() {
    // The legacy mode keeps rmcp's list verbatim: narrowing it would change
    // what `initialize` may agree to, which every committed baseline pins.
    assert_eq!(
        EverythingServer::new()
            .supported_protocol_versions()
            .as_ref(),
        ProtocolVersion::KNOWN_VERSIONS
    );
    assert_eq!(
        EverythingServer::serving(ServedRevision::V2026_07_28)
            .supported_protocol_versions()
            .as_ref(),
        [ProtocolVersion::V_2026_07_28]
    );
}

#[test]
fn the_stateless_revision_logs_only_for_a_request_that_asked() {
    // LOG-008 is a MUST NOT, and the default is silence: `logging/setLevel` is
    // gone at this revision, so a session threshold would be one nobody set.
    let server = EverythingServer::serving(ServedRevision::V2026_07_28);
    assert!(
        !server.log_permitted_for(None, LoggingLevel::Critical),
        "a request that asked for nothing gets nothing, at any level"
    );
    assert!(server.log_permitted_for(Some(LoggingLevel::Info), LoggingLevel::Info));
    assert!(server.log_permitted_for(Some(LoggingLevel::Info), LoggingLevel::Error));
    assert!(
        !server.log_permitted_for(Some(LoggingLevel::Error), LoggingLevel::Info),
        "and the level it asked for still filters"
    );
}

#[test]
fn the_legacy_revision_ignores_a_per_request_level() {
    // The field is a `2026-07-28` addition. Honouring it at `2025-11-25` would
    // let a client silently bypass the threshold `logging/setLevel` set, which
    // is the mechanism that revision actually defines.
    let server = EverythingServer::new();
    *server.log_level.lock().unwrap() = LoggingLevel::Error;
    assert!(!server.log_permitted_for(Some(LoggingLevel::Debug), LoggingLevel::Info));
}
