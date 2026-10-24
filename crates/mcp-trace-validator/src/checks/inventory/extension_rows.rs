// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The extensions' rows of the check inventory (ADR-0023): the checks the
//! tasks and skills registries name, reported beside the core verdict.

use super::Check;
use crate::checks::extensions::{skills, tasks};

macro_rules! check {
    ($id:literal, $run:path) => {
        Check { id: $id, run: $run }
    };
}

/// The extensions' rows, in stable order: tasks, then skills.
pub(super) const ROWS: &[Check] = &[
    check!(
        "tasks.create-requires-declaration",
        tasks::create_requires_declaration
    ),
    check!(
        "tasks.create-result-discriminator",
        tasks::create_result_discriminator
    ),
    check!(
        "tasks.task-result-type-only-on-create",
        tasks::task_result_type_only_on_create
    ),
    check!("tasks.task-fields", tasks::task_fields),
    check!(
        "tasks.input-required-carries-requests",
        tasks::input_required_carries_requests
    ),
    check!(
        "tasks.completed-carries-result",
        tasks::completed_carries_result
    ),
    check!("tasks.failed-carries-error", tasks::failed_carries_error),
    check!(
        "tasks.failed-not-tool-result",
        tasks::failed_not_tool_result
    ),
    check!("tasks.failed-status-message", tasks::failed_status_message),
    check!("tasks.results-complete", tasks::results_complete),
    check!("tasks.ack-empty", tasks::ack_empty),
    check!(
        "tasks.input-request-keys-stable",
        tasks::input_request_keys_stable
    ),
    check!("tasks.update-answers-once", tasks::update_answers_once),
    check!(
        "tasks.no-cancelled-notification",
        tasks::no_cancelled_notification
    ),
    check!(
        "tasks.listen-requires-declaration",
        tasks::listen_requires_declaration
    ),
    check!(
        "tasks.no-progress-on-task-stream",
        tasks::no_progress_on_task_stream
    ),
    check!("tasks.mcp-name-header", tasks::mcp_name_header),
    check!(
        "tasks.methods-require-declaration",
        tasks::methods_require_declaration
    ),
    check!("skills.list-implemented", skills::list_implemented),
    check!("skills.get-implemented", skills::get_implemented),
    check!(
        "skills.directory-read-implemented",
        skills::directory_read_implemented
    ),
    check!(
        "skills.directory-read-declared",
        skills::directory_read_declared
    ),
    check!("skills.declares-resources", skills::declares_resources),
    check!(
        "skills.entry-uri-is-skill-md",
        skills::entries::entry_uri_is_skill_md
    ),
    check!(
        "skills.skill-md-frontmatter",
        skills::content::skill_md_frontmatter
    ),
    check!("skills.skill-scheme", skills::entries::skill_scheme),
    check!(
        "skills.final-segment-is-name",
        skills::entries::final_segment_is_name
    ),
    check!(
        "skills.name-follows-naming-rules",
        skills::entries::name_follows_naming_rules
    ),
    check!(
        "skills.authority-reg-name",
        skills::entries::authority_reg_name
    ),
    check!("skills.prefix-segments", skills::entries::prefix_segments),
    check!(
        "skills.skill-md-mime-type",
        skills::content::skill_md_mime_type
    ),
    check!(
        "skills.skill-md-resource-name",
        skills::content::skill_md_resource_name
    ),
    check!(
        "skills.skill-md-resource-description",
        skills::content::skill_md_resource_description
    ),
    check!(
        "skills.frontmatter-identical",
        skills::content::frontmatter_identical
    ),
    check!(
        "skills.listing-names-unique",
        skills::entries::listing_names_unique
    ),
    check!(
        "skills.entry-resources-form",
        skills::entries::entry_resources_form
    ),
    check!(
        "skills.manifest-lists-skill-md",
        skills::entries::manifest_lists_skill_md
    ),
    check!(
        "skills.manifest-within-skill",
        skills::entries::manifest_within_skill
    ),
    check!(
        "skills.manifest-entry-size",
        skills::entries::manifest_entry_size
    ),
    check!("skills.within-limits", skills::entries::within_limits),
    check!("skills.list-result-complete", skills::list_result_complete),
    check!("skills.list-cacheable", skills::list_cacheable),
    check!("skills.get-result-complete", skills::get_result_complete),
    check!(
        "skills.get-uri-names-skill-md",
        skills::get_uri_names_skill_md
    ),
    check!(
        "skills.get-unknown-invalid-params",
        skills::get_unknown_invalid_params
    ),
    check!("skills.get-cacheable", skills::get_cacheable),
    check!(
        "skills.directory-result-complete",
        skills::directory_result_complete
    ),
    check!("skills.directory-cacheable", skills::directory_cacheable),
    check!(
        "skills.directory-read-invalid-params",
        skills::directory_read_invalid_params
    ),
    check!(
        "skills.directory-read-every-directory",
        skills::directory_read_every_directory
    ),
];
