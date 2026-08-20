// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The check inventory itself: the registered [`Check`] list, lookup by id, and
//! the ledger of check ids the registry names ahead of their implementation.
//!
//! Split from [`super`] so the type definitions stay readable beside their
//! contract while the list — which grows with every extracted area — lives on
//! its own.

use super::{Check, base, lifecycle, negotiation, prompts, resources, tools, transport, utilities};

/// One registration row. The list is long and perfectly uniform, so the literal
/// `Check { id: …, run: … }` form cost four lines each and pushed the file past
/// the 500-line cap for no gain in reviewability — the pair *is* the row.
macro_rules! check {
    ($id:literal, $run:path) => {
        Check { id: $id, run: $run }
    };
}

/// The `2025-11-25` half of the inventory.
const SHIPPED: &[Check] = &[
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
        "transport.client-post-accept-header",
        transport::client_post_accept_header
    ),
    check!(
        "transport.client-get-accept-header",
        transport::client_get_accept_header
    ),
    check!(
        "transport.client-messages-use-post",
        transport::client_messages_use_post
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
];

/// Concatenates the two halves at compile time, so each may live in its own file
/// while `ALL` stays one `&[Check]` — the shape every caller and the coverage
/// invariants read. `Check` is `Copy`, which is what makes the const loop legal.
const fn concatenated<const N: usize>(first: &[Check], second: &[Check]) -> [Check; N] {
    let mut out = [first[0]; N];
    let mut index = 0;
    while index < first.len() {
        out[index] = first[index];
        index += 1;
    }
    let mut offset = 0;
    while offset < second.len() {
        out[first.len() + offset] = second[offset];
        offset += 1;
    }
    out
}

/// The backing storage for [`ALL`].
static EVERY: [Check; SHIPPED.len() + draft_rows::DRAFT.len()] =
    concatenated(SHIPPED, draft_rows::DRAFT);

/// Every check implemented by this build, in stable order.
pub static ALL: &[Check] = &EVERY;
/// Looks up a check by its stable ID.
#[must_use]
pub fn find(id: &str) -> Option<&'static Check> {
    ALL.iter().find(|check| check.id == id)
}

mod draft_rows;

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
