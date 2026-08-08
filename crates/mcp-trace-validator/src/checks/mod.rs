// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The check inventory.
//!
//! A *check* is a pure function from a [`TraceContext`] to findings, registered under a
//! stable ID that requirement-registry entries reference. The contract for every check:
//!
//! - **Falsifiable**: the corpus contains at least one trace it passes and one it fails
//!   (enforced by the corpus invariant test).
//! - **Deterministic**: findings are emitted in event order with stable details.
//! - **Lenient input, precise output**: checks never refuse malformed messages — they
//!   report them.

mod base;
#[cfg(feature = "draft-2026-07-28")]
mod draft;
mod lifecycle;
mod negotiation;
mod prompts;
mod resources;
mod support;
mod tools;
mod transport;
mod utilities;

use crate::context::TraceContext;
use crate::report::Finding;

/// A check function: examines the trace, pushes findings into the sink.
type CheckFn = fn(&TraceContext<'_>, &mut FindingSink);

/// A registered check.
#[derive(Debug, Clone, Copy)]
pub struct Check {
    /// Stable check identifier referenced by registry entries (e.g.
    /// `lifecycle.first-interaction-initialize`).
    pub id: &'static str,
    run: CheckFn,
}

impl Check {
    /// Runs the check, returning its findings tagged with this check's ID.
    #[must_use]
    pub fn run(&self, context: &TraceContext<'_>) -> Vec<Finding> {
        let mut sink = FindingSink {
            check: self.id,
            findings: Vec::new(),
        };
        (self.run)(context, &mut sink);
        sink.findings
    }
}

/// Collects findings on behalf of one check, stamping each with the check ID.
#[derive(Debug)]
pub struct FindingSink {
    check: &'static str,
    findings: Vec<Finding>,
}

impl FindingSink {
    /// Records a finding at an event (`seq`) with an actionable detail sentence.
    pub fn push(&mut self, seq: Option<u64>, detail: String) {
        self.findings.push(Finding {
            check: self.check.to_owned(),
            seq,
            detail,
        });
    }
}

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
];

/// Looks up a check by its stable ID.
#[must_use]
pub fn find(id: &str) -> Option<&'static Check> {
    ALL.iter().find(|check| check.id == id)
}

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
                        find(check).is_some(),
                        "{}: references unimplemented check {check}",
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
    }
}
