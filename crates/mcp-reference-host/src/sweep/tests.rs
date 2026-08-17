// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the pure parts: template substitution, argument synthesis, and
//! the report's accounting.
//!
//! [`run`] itself needs a live server and gets one in `tests/sweep.rs`, which
//! drives the whole non-tool surface against the real everything server and
//! asserts each step by name — a count would pass while the wrong one was
//! missing.

use super::*;

/// A prompt declaring `names`, built through the constructor because
/// `Prompt` is `#[non_exhaustive]`.
fn prompt_with(names: &[&str]) -> Prompt {
    let mut prompt = Prompt::new("p", Some("a prompt"), None);
    prompt.arguments = Some(
        names
            .iter()
            .map(|name| rmcp::model::PromptArgument::new(*name))
            .collect(),
    );
    prompt
}

#[test]
fn a_template_variable_is_filled_and_a_plain_uri_is_not() {
    assert_eq!(
        substitute("test://template/{id}/data").as_deref(),
        Some("test://template/1/data")
    );
    assert_eq!(
        substitute("test://a/{x}/b/{y}").as_deref(),
        Some("test://a/1/b/1"),
        "every variable is filled, not just the first"
    );
    // Nothing to substitute: reading it would repeat a resources/list entry.
    assert_eq!(substitute("test://static-text"), None);
}

#[test]
fn operator_forms_and_malformed_templates_are_left_alone() {
    // `{+var}`, `{#var}`, `{?var}` change what the value means; filling one
    // would synthesize a URI the server never published.
    for template in [
        "test://a/{+path}",
        "test://a{?query}",
        "test://a/{#frag}",
        "test://a/{a,b}",
    ] {
        assert_eq!(substitute(template), None, "{template}");
    }
    // Unbalanced or empty braces are not templates this can safely fill.
    assert_eq!(substitute("test://a/{id"), None);
    assert_eq!(substitute("test://a/{}/b"), None);
}

#[test]
fn a_resource_argument_gets_a_uri_the_server_listed() {
    // The everything server embeds this value verbatim, so a placeholder
    // would manufacture a RES-004 finding against the server for a URI the
    // client invented.
    let arguments = prompt_arguments(
        &prompt_with(&["resourceUri", "arg1"]),
        Some("test://static-text"),
    );
    assert_eq!(arguments["resourceUri"], "test://static-text");
    assert_eq!(arguments["arg1"], "probe");
}

#[test]
fn without_a_discovered_resource_every_argument_falls_back() {
    // A server publishing no resources cannot supply one; the placeholder is
    // then the only honest answer, and the prompt's own error is the server's
    // to give.
    let arguments = prompt_arguments(&prompt_with(&["resourceUri"]), None);
    assert_eq!(arguments["resourceUri"], "probe");
}

#[test]
fn a_prompt_with_no_arguments_is_called_with_none() {
    let mut bare = Prompt::new("p", Some("a prompt"), None);
    bare.arguments = None;
    assert!(prompt_arguments(&bare, Some("test://x")).is_empty());
}

#[test]
fn the_report_counts_only_the_steps_that_drew_errors() {
    let mut report = SweepReport::default();
    report.record("resources/list", Ok("2 resource(s)".to_owned()));
    report.record("resources/read test://gone", Err("-32602".to_owned()));
    assert_eq!(report.errors(), 1);
    assert_eq!(report.steps.len(), 2);
    assert_eq!(report.steps[1].what, "resources/read test://gone");
}
