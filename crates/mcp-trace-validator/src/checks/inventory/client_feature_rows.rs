// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The client-feature pages' rows of the check inventory, shared by both
//! revisions (see `checks::client_features`).

use super::Check;
use crate::checks::client_features::{elicitation, roots, sampling, url_flow};

macro_rules! check {
    ($id:literal, $run:path) => {
        Check { id: $id, run: $run }
    };
}

/// The rows, in stable order.
pub(super) const ROWS: &[Check] = &[
    check!(
        "elicitation.client-declares-capability",
        elicitation::client_declares_capability
    ),
    check!("elicitation.mode-supported", elicitation::mode_supported),
    check!(
        "elicitation.request-parameters",
        elicitation::request_parameters
    ),
    check!(
        "elicitation.form-schema-present",
        elicitation::form_schema_present
    ),
    check!("elicitation.url-parameters", elicitation::url_parameters),
    check!(
        "elicitation.url-parameters-with-id",
        elicitation::url_parameters_with_id
    ),
    check!("elicitation.url-valid", elicitation::url_valid),
    check!(
        "elicitation.response-matches-schema",
        elicitation::response_matches_schema
    ),
    check!(
        "elicitation.undeclared-mode-rejected",
        elicitation::undeclared_mode_rejected
    ),
    check!(
        "elicitation.complete-notification-id",
        url_flow::complete_notification_id
    ),
    check!(
        "elicitation.url-required-error-lists",
        url_flow::url_required_error_lists
    ),
    check!(
        "elicitation.url-required-error-url-mode",
        url_flow::url_required_error_url_mode
    ),
    check!(
        "sampling.client-declares-capability",
        sampling::client_declares_capability
    ),
    check!("sampling.tools-declared", sampling::tools_declared),
    check!(
        "sampling.include-context-avoided",
        sampling::include_context_avoided
    ),
    check!(
        "sampling.include-context-declared",
        sampling::include_context_declared
    ),
    check!("sampling.tool-results-only", sampling::tool_results_only),
    check!("sampling.tool-use-answered", sampling::tool_use_answered),
    check!("sampling.message-shape", sampling::message_shape),
    check!(
        "sampling.tool-choice-required-honoured",
        sampling::tool_choice_required_honoured
    ),
    check!(
        "sampling.tool-choice-none-honoured",
        sampling::tool_choice_none_honoured
    ),
    check!(
        "sampling.malformed-tool-messages-rejected",
        sampling::malformed_tool_messages_rejected
    ),
    check!(
        "roots.client-declares-capability",
        roots::client_declares_capability
    ),
    check!("roots.uri-file-scheme", roots::uri_file_scheme),
    check!("roots.capability-checked", roots::capability_checked),
    check!("roots.unsupported-rejected", roots::unsupported_rejected),
];
