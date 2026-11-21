// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The feature pages' error-handling rows of the check inventory, most of
//! them shared by both revisions (see `checks::rejections`).

use super::Check;
use crate::checks::rejections;

macro_rules! check {
    ($id:literal, $run:path) => {
        Check { id: $id, run: $run }
    };
}

/// The rows, in stable order.
pub(super) const ROWS: &[Check] = &[
    check!(
        "logging.set-level-invalid-rejected",
        rejections::logging::set_level_invalid_rejected
    ),
    check!(
        "prompts.get-error-codes",
        rejections::prompts::get_error_codes
    ),
    check!(
        "resources.not-found-code",
        rejections::resources::not_found_code
    ),
    check!(
        "resources.missing-resource-code",
        rejections::resources::missing_resource_code
    ),
    check!(
        "completion.error-codes",
        rejections::completion::error_codes
    ),
    check!(
        "pagination.malformed-cursor-rejected",
        rejections::pagination::malformed_cursor_rejected
    ),
];
