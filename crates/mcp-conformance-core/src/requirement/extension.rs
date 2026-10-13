// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Extension clauses: requirements an official MCP extension states, reported
//! beside a revision's core clauses and never scored with them (ADR-0023).
//!
//! An extension is optional by definition — "Extensions are always disabled by
//! default and require explicit opt-in" (the specification's extensions
//! overview) — so its clauses cannot decide whether an implementation conforms
//! to the protocol. The official suite draws the same line: its requirement sets
//! run an extension's scenarios under `not_scored` with the reason `extension`,
//! reported and never counted. A [`Requirement`](super::Requirement) whose
//! `extension` names one belongs to that extension's registry, and a
//! [`Registry`](super::Registry) keeps such entries apart from its core ones, so
//! nothing that reads the core clauses can count them by accident.
//!
//! The text of an extension is not published under the specification's dated
//! directories: each official extension keeps its own repository
//! (`modelcontextprotocol/ext-*`), so where a clause is published depends on
//! the extension, not on the protocol revision.

/// Where each official extension the built-in registry describes publishes the
/// specification its entries quote: an entry's `source.section` is appended to
/// this prefix.
///
/// The tasks extension publishes a rendered site whose heading ids anchor each
/// section. The skills extension's source of truth (`specification/stable/skills.mdx`)
/// has no rendered publication — its documentation site is not deployed — so its
/// sections name the file, and the link opens it where the repository serves it.
const PUBLISHED: [(&str, &str); 2] = [
    (
        "io.modelcontextprotocol/tasks",
        "https://modelcontextprotocol.github.io/ext-tasks/",
    ),
    (
        "io.modelcontextprotocol/skills",
        "https://github.com/modelcontextprotocol/ext-skills/blob/main/",
    ),
];

/// Whether `id` is shaped like an extension identifier:
/// `{vendor-prefix}/{extension-name}`, as `io.modelcontextprotocol/tasks` is —
/// a non-empty prefix and name around a `/`, and no whitespace.
pub(super) fn is_identifier(id: &str) -> bool {
    id.split_once('/').is_some_and(|(prefix, name)| {
        !prefix.is_empty() && !name.is_empty() && !name.ends_with('/')
    }) && !id.chars().any(char::is_whitespace)
}

/// The published page for `section` of extension `id`, anchor included; `None`
/// for an extension this build does not know where to find.
pub(super) fn published_url(id: &str, section: &str) -> Option<String> {
    PUBLISHED
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, base)| format!("{base}{section}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_have_a_prefix_and_a_name() {
        for good in [
            "io.modelcontextprotocol/tasks",
            "io.modelcontextprotocol/auth/dpop",
            "com.example/x",
        ] {
            assert!(is_identifier(good), "{good}");
        }
        for bad in [
            "",
            "tasks",
            "/tasks",
            "io.modelcontextprotocol/",
            "io.modelcontextprotocol/tasks/",
            "io.modelcontextprotocol/my tasks",
        ] {
            assert!(!is_identifier(bad), "{bad:?}");
        }
    }

    #[test]
    fn known_extensions_link_their_own_publication() {
        assert_eq!(
            published_url(
                "io.modelcontextprotocol/tasks",
                "specification/2026-07-28/tasks#task-polling"
            )
            .as_deref(),
            Some(
                "https://modelcontextprotocol.github.io/ext-tasks/specification/2026-07-28/tasks#task-polling"
            )
        );
        assert_eq!(
            published_url(
                "io.modelcontextprotocol/skills",
                "specification/stable/skills.mdx"
            )
            .as_deref(),
            Some(
                "https://github.com/modelcontextprotocol/ext-skills/blob/main/specification/stable/skills.mdx"
            )
        );
        assert_eq!(published_url("com.example/x", "a#b"), None);
    }
}
