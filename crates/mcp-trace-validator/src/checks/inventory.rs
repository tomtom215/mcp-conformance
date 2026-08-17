// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The check inventory itself: the registered [`Check`] list, lookup by id, and
//! the ledger of check ids the registry names ahead of their implementation.
//!
//! Split from [`super`] so the type definitions stay readable beside their
//! contract while the list — which grows with every extracted area — lives on
//! its own.

#[cfg(feature = "draft-2026-07-28")]
use super::draft;
use super::{Check, base, lifecycle, negotiation, prompts, resources, tools, transport, utilities};

/// One registration row. The list is long and perfectly uniform, so the literal
/// `Check { id: …, run: … }` form cost four lines each and pushed the file past
/// the 500-line cap for no gain in reviewability — the pair *is* the row.
macro_rules! check {
    ($id:literal, $run:path) => {
        Check { id: $id, run: $run }
    };
}

/// Every check implemented by this build, in stable order.
pub static ALL: &[Check] = &[
    check!("base.request-id-type", base::request_id_type),
    check!("base.request-id-not-null", base::request_id_not_null),
    check!("base.request-id-unique", base::request_id_unique),
    check!("base.result-id-matches", base::result_id_matches),
    check!("base.notification-no-id", base::notification_no_id),
    check!("base.error-shape", base::error_shape),
    check!("base.error-code-integer", base::error_code_integer),
    check!("base.jsonrpc-version", base::jsonrpc_version),
    check!("base.error-id-matches", base::error_id_matches),
    check!(
        "lifecycle.first-interaction-initialize",
        lifecycle::first_interaction_initialize
    ),
    check!("lifecycle.initialize-params", lifecycle::initialize_params),
    check!(
        "lifecycle.initialized-notification",
        lifecycle::initialized_notification
    ),
    check!(
        "lifecycle.client-requests-before-init-response",
        lifecycle::client_requests_before_init_response
    ),
    check!(
        "lifecycle.server-requests-before-initialized",
        lifecycle::server_requests_before_initialized
    ),
    check!(
        "lifecycle.initialize-result-version",
        lifecycle::initialize_result_version
    ),
    check!(
        "lifecycle.initialize-result-shape",
        lifecycle::initialize_result_shape
    ),
    check!("base.meta-key-format", base::meta_key_format),
    check!("base.result-field", base::result_field),
    check!(
        "lifecycle.initialize-protocol-version",
        lifecycle::initialize_protocol_version
    ),
    check!(
        "lifecycle.negotiated-capabilities-only",
        negotiation::negotiated_capabilities_only
    ),
    check!(
        "transport.stdio-server-output-valid",
        transport::stdio_server_output_valid
    ),
    check!(
        "transport.stdio-client-input-valid",
        transport::stdio_client_input_valid
    ),
    check!(
        "transport.session-id-visible-ascii",
        transport::session_id_visible_ascii
    ),
    check!("transport.session-id-echoed", transport::session_id_echoed),
    check!(
        "transport.protocol-version-header",
        transport::protocol_version_header
    ),
    check!(
        "transport.protocol-version-negotiated",
        transport::protocol_version_negotiated
    ),
    check!(
        "transport.http-post-single-message",
        transport::http_post_single_message
    ),
    check!(
        "transport.client-accept-header",
        transport::client_accept_header
    ),
    check!(
        "transport.success-content-type",
        transport::success_content_type
    ),
    check!("tools.capability-declared", tools::capability_declared),
    check!("tools.input-schema-object", tools::input_schema_object),
    check!("tools.name-length", tools::name_length),
    check!("tools.name-charset", tools::name_charset),
    check!("tools.name-unique", tools::name_unique),
    check!(
        "tools.embedded-resource-capability",
        tools::embedded_resource_capability
    ),
    check!(
        "tools.structured-content-text",
        tools::structured_content_text
    ),
    check!(
        "tools.output-schema-structured-result",
        tools::output_schema_structured_result
    ),
    check!(
        "resources.capability-declared",
        resources::capability_declared
    ),
    check!(
        "resources.uri-scheme-rfc3986",
        resources::uri_scheme_rfc3986
    ),
    check!("resources.blob-base64", resources::blob_base64),
    check!("prompts.capability-declared", prompts::capability_declared),
    check!(
        "prompts.image-content-encoding",
        prompts::image_content_encoding
    ),
    check!(
        "prompts.audio-content-encoding",
        prompts::audio_content_encoding
    ),
    check!(
        "prompts.embedded-resource-shape",
        prompts::embedded_resource_shape
    ),
    check!("prompts.arguments-validated", prompts::arguments_validated),
    check!(
        "logging.capability-declared",
        utilities::logging_capability_declared
    ),
    check!(
        "completion.capability-declared",
        utilities::completion_capability_declared
    ),
    check!("pagination.cursor-opacity", utilities::cursor_opacity),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "base.result-type-present",
        draft::envelope::result_type_present
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "base.request-id-unique-in-flight",
        draft::envelope::request_id_unique_in_flight
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "base.error-code-legacy-subrange",
        draft::envelope::error_code_legacy_subrange
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "base.error-code-reserved-subrange",
        draft::envelope::error_code_reserved_subrange
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "base.error-code-withdrawn",
        draft::envelope::error_code_withdrawn
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "base.error-code-application-range",
        draft::envelope::error_code_application_range
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.required-request-fields",
        draft::meta::required_request_fields
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.missing-required-field-rejected",
        draft::meta::missing_required_field_rejected
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.missing-required-field-http-status",
        draft::meta::missing_required_field_http_status
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.missing-capability-error",
        draft::meta::missing_capability_error
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.missing-capability-http-status",
        draft::meta::missing_capability_http_status
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.no-undeclared-capability-reliance",
        draft::meta::no_undeclared_capability_reliance
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.subscription-id-present",
        draft::meta::subscription_id_present
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "meta.trace-context-format",
        draft::meta::trace_context_format
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.protocol-version-header-present",
        draft::transport::protocol_version_header_present
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.protocol-version-header-matches-body",
        draft::transport::protocol_version_header_matches_body
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.request-metadata-headers",
        draft::transport::request_metadata_headers
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.client-no-responses",
        draft::transport::client_no_responses
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.no-independent-server-requests",
        draft::transport::no_independent_server_requests
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.accel-buffering-header",
        draft::transport::accel_buffering_header
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.no-messages-after-cancellation",
        draft::transport::no_messages_after_cancellation
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.version-mismatch-rejected",
        draft::transport::version_mismatch_rejected
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.invalid-param-header-rejected",
        draft::transport::invalid_param_header_rejected
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.header-mismatch-status",
        draft::transport::header_mismatch_status
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.header-body-match-validated",
        draft::transport::header_body_match_validated
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.unsupported-version-error",
        draft::transport::unsupported_version_error
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.unsupported-version-status",
        draft::transport::unsupported_version_status
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.unknown-method-404",
        draft::transport::unknown_method_404
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.header-value-encoding",
        draft::transport::header_value_encoding
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.sentinel-marker-case",
        draft::transport::sentinel_marker_case
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.sentinel-pattern-encoded",
        draft::transport::sentinel_pattern_encoded
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.x-mcp-header-mirrored",
        draft::transport::x_mcp_header_mirrored
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.x-mcp-header-name-valid",
        draft::transport::x_mcp_header_name_valid
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!("discover.implemented", draft::discovery::implemented),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "discover.dual-era-probe-first",
        draft::discovery::dual_era_probe_first
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "versioning.retry-uses-supported-version",
        draft::versioning::retry_uses_supported_version
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "versioning.extension-identifier-format",
        draft::versioning::extension_identifier_format
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "versioning.initialize-error-names-versions",
        draft::versioning::initialize_error_names_versions
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.cancel-notification-references-request",
        draft::transport::cancel_notification_references_request
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "transport.no-messages-after-cancel-notification",
        draft::transport::no_messages_after_cancel_notification
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.input-required-supported-methods",
        draft::mrtr::input_required_supported_methods
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.input-request-methods",
        draft::mrtr::input_request_methods
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.input-required-has-content",
        draft::mrtr::input_required_has_content
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.retry-carries-input-responses",
        draft::mrtr::retry_carries_input_responses
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.request-state-echoed",
        draft::mrtr::request_state_echoed
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.no-unsolicited-request-state",
        draft::mrtr::no_unsolicited_request_state
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!("mrtr.retry-id-differs", draft::mrtr::retry_id_differs),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.request-state-scoped-to-retry",
        draft::mrtr::request_state_scoped_to_retry
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "mrtr.missing-input-reasked",
        draft::mrtr::missing_input_reasked
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "subscriptions.only-requested-notifications",
        draft::subscriptions::only_requested_notifications
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "subscriptions.acknowledgment-first",
        draft::subscriptions::acknowledgment_first
    ),
    #[cfg(feature = "draft-2026-07-28")]
    check!(
        "subscriptions.graceful-close-result-empty",
        draft::subscriptions::graceful_close_result_empty
    ),
];

/// Looks up a check by its stable ID.
#[must_use]
pub fn find(id: &str) -> Option<&'static Check> {
    ALL.iter().find(|check| check.id == id)
}

#[cfg(test)]
mod planned;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn check_ids_are_unique() {
        let mut seen = HashSet::new();
        for check in ALL {
            assert!(seen.insert(check.id), "duplicate check id {}", check.id);
        }
    }
}
