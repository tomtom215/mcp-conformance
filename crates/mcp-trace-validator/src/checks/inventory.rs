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

/// Every check implemented by this build, in stable order.
pub static ALL: &[Check] = &[
    Check {
        id: "base.request-id-type",
        run: base::request_id_type,
    },
    Check {
        id: "base.request-id-not-null",
        run: base::request_id_not_null,
    },
    Check {
        id: "base.request-id-unique",
        run: base::request_id_unique,
    },
    Check {
        id: "base.result-id-matches",
        run: base::result_id_matches,
    },
    Check {
        id: "base.notification-no-id",
        run: base::notification_no_id,
    },
    Check {
        id: "base.error-shape",
        run: base::error_shape,
    },
    Check {
        id: "base.error-code-integer",
        run: base::error_code_integer,
    },
    Check {
        id: "base.jsonrpc-version",
        run: base::jsonrpc_version,
    },
    Check {
        id: "base.error-id-matches",
        run: base::error_id_matches,
    },
    Check {
        id: "lifecycle.first-interaction-initialize",
        run: lifecycle::first_interaction_initialize,
    },
    Check {
        id: "lifecycle.initialize-params",
        run: lifecycle::initialize_params,
    },
    Check {
        id: "lifecycle.initialized-notification",
        run: lifecycle::initialized_notification,
    },
    Check {
        id: "lifecycle.client-requests-before-init-response",
        run: lifecycle::client_requests_before_init_response,
    },
    Check {
        id: "lifecycle.server-requests-before-initialized",
        run: lifecycle::server_requests_before_initialized,
    },
    Check {
        id: "lifecycle.initialize-result-version",
        run: lifecycle::initialize_result_version,
    },
    Check {
        id: "lifecycle.initialize-result-shape",
        run: lifecycle::initialize_result_shape,
    },
    Check {
        id: "base.meta-key-format",
        run: base::meta_key_format,
    },
    Check {
        id: "base.result-field",
        run: base::result_field,
    },
    Check {
        id: "lifecycle.initialize-protocol-version",
        run: lifecycle::initialize_protocol_version,
    },
    Check {
        id: "lifecycle.negotiated-capabilities-only",
        run: negotiation::negotiated_capabilities_only,
    },
    Check {
        id: "transport.stdio-server-output-valid",
        run: transport::stdio_server_output_valid,
    },
    Check {
        id: "transport.stdio-client-input-valid",
        run: transport::stdio_client_input_valid,
    },
    Check {
        id: "transport.session-id-visible-ascii",
        run: transport::session_id_visible_ascii,
    },
    Check {
        id: "transport.session-id-echoed",
        run: transport::session_id_echoed,
    },
    Check {
        id: "transport.protocol-version-header",
        run: transport::protocol_version_header,
    },
    Check {
        id: "transport.protocol-version-negotiated",
        run: transport::protocol_version_negotiated,
    },
    Check {
        id: "transport.http-post-single-message",
        run: transport::http_post_single_message,
    },
    Check {
        id: "transport.client-accept-header",
        run: transport::client_accept_header,
    },
    Check {
        id: "transport.success-content-type",
        run: transport::success_content_type,
    },
    Check {
        id: "tools.capability-declared",
        run: tools::capability_declared,
    },
    Check {
        id: "tools.input-schema-object",
        run: tools::input_schema_object,
    },
    Check {
        id: "tools.name-length",
        run: tools::name_length,
    },
    Check {
        id: "tools.name-charset",
        run: tools::name_charset,
    },
    Check {
        id: "tools.name-unique",
        run: tools::name_unique,
    },
    Check {
        id: "tools.embedded-resource-capability",
        run: tools::embedded_resource_capability,
    },
    Check {
        id: "tools.structured-content-text",
        run: tools::structured_content_text,
    },
    Check {
        id: "tools.output-schema-structured-result",
        run: tools::output_schema_structured_result,
    },
    Check {
        id: "resources.capability-declared",
        run: resources::capability_declared,
    },
    Check {
        id: "resources.uri-scheme-rfc3986",
        run: resources::uri_scheme_rfc3986,
    },
    Check {
        id: "resources.blob-base64",
        run: resources::blob_base64,
    },
    Check {
        id: "prompts.capability-declared",
        run: prompts::capability_declared,
    },
    Check {
        id: "prompts.image-content-encoding",
        run: prompts::image_content_encoding,
    },
    Check {
        id: "prompts.audio-content-encoding",
        run: prompts::audio_content_encoding,
    },
    Check {
        id: "prompts.embedded-resource-shape",
        run: prompts::embedded_resource_shape,
    },
    Check {
        id: "prompts.arguments-validated",
        run: prompts::arguments_validated,
    },
    Check {
        id: "logging.capability-declared",
        run: utilities::logging_capability_declared,
    },
    Check {
        id: "completion.capability-declared",
        run: utilities::completion_capability_declared,
    },
    Check {
        id: "pagination.cursor-opacity",
        run: utilities::cursor_opacity,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "base.result-type-present",
        run: draft::envelope::result_type_present,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "base.request-id-unique-in-flight",
        run: draft::envelope::request_id_unique_in_flight,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "base.error-code-legacy-subrange",
        run: draft::envelope::error_code_legacy_subrange,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "base.error-code-reserved-subrange",
        run: draft::envelope::error_code_reserved_subrange,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "base.error-code-withdrawn",
        run: draft::envelope::error_code_withdrawn,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "base.error-code-application-range",
        run: draft::envelope::error_code_application_range,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.required-request-fields",
        run: draft::meta::required_request_fields,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.missing-required-field-rejected",
        run: draft::meta::missing_required_field_rejected,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.missing-required-field-http-status",
        run: draft::meta::missing_required_field_http_status,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.missing-capability-error",
        run: draft::meta::missing_capability_error,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.missing-capability-http-status",
        run: draft::meta::missing_capability_http_status,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.no-undeclared-capability-reliance",
        run: draft::meta::no_undeclared_capability_reliance,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.subscription-id-present",
        run: draft::meta::subscription_id_present,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "meta.trace-context-format",
        run: draft::meta::trace_context_format,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.protocol-version-header-present",
        run: draft::transport::protocol_version_header_present,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.protocol-version-header-matches-body",
        run: draft::transport::protocol_version_header_matches_body,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.request-metadata-headers",
        run: draft::transport::request_metadata_headers,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.client-no-responses",
        run: draft::transport::client_no_responses,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.no-independent-server-requests",
        run: draft::transport::no_independent_server_requests,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.accel-buffering-header",
        run: draft::transport::accel_buffering_header,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.no-messages-after-cancellation",
        run: draft::transport::no_messages_after_cancellation,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.version-mismatch-rejected",
        run: draft::transport::version_mismatch_rejected,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.invalid-param-header-rejected",
        run: draft::transport::invalid_param_header_rejected,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.header-mismatch-status",
        run: draft::transport::header_mismatch_status,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.header-body-match-validated",
        run: draft::transport::header_body_match_validated,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.unsupported-version-error",
        run: draft::transport::unsupported_version_error,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.unknown-method-404",
        run: draft::transport::unknown_method_404,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.header-value-encoding",
        run: draft::transport::header_value_encoding,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.sentinel-marker-case",
        run: draft::transport::sentinel_marker_case,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.sentinel-pattern-encoded",
        run: draft::transport::sentinel_pattern_encoded,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.x-mcp-header-mirrored",
        run: draft::transport::x_mcp_header_mirrored,
    },
    #[cfg(feature = "draft-2026-07-28")]
    Check {
        id: "transport.x-mcp-header-name-valid",
        run: draft::transport::x_mcp_header_name_valid,
    },
];

/// Looks up a check by its stable ID.
#[must_use]
pub fn find(id: &str) -> Option<&'static Check> {
    ALL.iter().find(|check| check.id == id)
}

/// Whether `check` is a committed-but-unimplemented id (see `PLANNED`).
#[cfg(test)]
// Const-evaluable only with the feature off, where the body is a literal `false`;
// with it on, the slice lookup is not const. Following the lint would make the
// signature depend on which features are enabled.
#[allow(clippy::missing_const_for_fn)]
fn is_planned(check: &str) -> bool {
    #[cfg(feature = "draft-2026-07-28")]
    let planned = PLANNED.contains(&check);
    #[cfg(not(feature = "draft-2026-07-28"))]
    let planned = {
        let _ = check;
        false
    };
    planned
}

#[cfg(all(test, feature = "draft-2026-07-28"))]
/// Checks a registry entry names that this build does not implement yet.
///
/// The engine reports such a requirement as `unsupported` — first-class in the
/// totals, listed with the missing id, outranking pass/fail — so an entry naming
/// one states something true (the clause is verified by this check) alongside a
/// visible build fact (the check is absent). That is the mechanism incremental
/// extraction is meant to use, and it is why an entry must never be given a
/// placeholder *exclusion* instead.
///
/// The list exists so the mechanism cannot hide a typo: a misspelled check id
/// would otherwise degrade silently to `unsupported` and read as planned work.
/// Every row is a commitment, and the test below retires each one the moment its
/// check lands.
///
/// Empty is the healthy state, and the state it is in now: every check the
/// registry names is implemented. The list stays because the next extracted area
/// will land its entries before its checks, and this is where that debt is
/// declared rather than left to read as a typo.
const PLANNED: &[&str] = &[];

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mcp_conformance_core::requirement::Verification;
    use std::collections::HashSet;

    #[test]
    fn check_ids_are_unique() {
        let mut seen = HashSet::new();
        for check in ALL {
            assert!(seen.insert(check.id), "duplicate check id {}", check.id);
        }
    }

    #[test]
    fn builtin_registry_and_check_inventory_cover_each_other_exactly() {
        // Every check the registry references exists, and every implemented check is
        // referenced — drift in either direction is a defect, not a warning.
        //
        // Driven from the registry *set*, not one revision: checks arrive with the
        // revision that needs them, so a `2026-07-28` check is referenced only by that
        // revision's entries. Both halves still bind — an implemented check no
        // revision names is dead code, and a named check nothing implements would be
        // reported `unsupported` rather than judged.
        let set = mcp_conformance_core::requirement::RegistrySet::builtin().unwrap();
        let mut referenced = HashSet::new();
        for requirement in set.requirements() {
            if let Verification::Checks { checks } = &requirement.verification {
                for check in checks {
                    assert!(
                        find(check).is_some() || is_planned(check),
                        "{}: references check {check}, which is neither implemented nor \
                         listed in PLANNED — a typo in a check id would otherwise be \
                         invisible, reported as `unsupported` rather than as a defect",
                        requirement.id
                    );
                    referenced.insert(check.clone());
                }
            }
        }
        for check in ALL {
            assert!(
                referenced.contains(check.id),
                "check {} is implemented but referenced by no requirement",
                check.id
            );
        }
        // The list retires itself: implementing a planned check without removing
        // its row fails here, so PLANNED can never quietly outlive its purpose.
        // Feature-gated with the data — without it, the revision that names these
        // is not described and nothing could reference them.
        #[cfg(feature = "draft-2026-07-28")]
        for planned in PLANNED {
            assert!(
                find(planned).is_none(),
                "check {planned} is implemented — remove it from PLANNED"
            );
            assert!(
                referenced.contains(*planned),
                "check {planned} is planned but no requirement references it"
            );
        }
    }
}
