// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A SARIF rule from a report row, and the two small functions a result's
//! level and fingerprint are built from.

use mcp_conformance_core::requirement::Actor;
use serde_json::{Value, json};

use super::NOT_SCORED;
use crate::report::{Outcome, RequirementReport};

/// The party a clause binds, as the registry spells it.
pub(super) const fn actor_token(actor: Actor) -> &'static str {
    match actor {
        Actor::Client => "client",
        Actor::Server => "server",
        Actor::Both => "both",
        // Foreign #[non_exhaustive] enum: a future party surfaces visibly.
        _ => "unknown",
    }
}

/// Adds `properties.actor` to `rule` when the row names one.
fn with_actor(mut rule: Value, actor: Option<Actor>) -> Value {
    if let Some(actor) = actor {
        rule["properties"]["actor"] = json!(actor_token(actor));
    }
    rule
}

pub(super) fn rule(
    rule_id: &str,
    row: &RequirementReport,
    revision: &str,
    extension: Option<&str>,
) -> Value {
    let rule = json!({
        "id": rule_id,
        "defaultConfiguration": { "level": extension.map_or_else(|| level(row.outcome), |_| NOT_SCORED) },
        "properties": {
            "tags": ["mcp", "conformance", row.level],
            "precision": "very-high",
            "revision": revision,
        },
    });
    let mut rule = with_actor(rule, row.actor);
    if let Some(extension) = extension {
        rule["properties"]["extension"] = json!(extension);
        rule["properties"]["scored"] = json!(false);
    }
    let (short, help) = row.source.as_ref().map_or_else(
        || (format!("{} ({})", row.id, row.level), None),
        |source| {
            (
                source.quote.clone(),
                Some((
                    json!({
                        "text": format!("{} ({}): \"{}\" — {}", row.id, row.level, source.quote, source.url),
                        "markdown": format!(
                            "**{} ({})**\n\n> {}\n\n[{}]({})",
                            row.id, row.level, source.quote, source.section, source.url
                        ),
                    }),
                    source.url.clone(),
                )),
            )
        },
    );
    rule["shortDescription"] = json!({ "text": short });
    if let Some((help, url)) = help {
        rule["fullDescription"] = json!({ "text": short });
        rule["help"] = help;
        rule["helpUri"] = json!(url);
    }
    rule
}

/// `text` with every run of ASCII digits replaced by `#`, so a finding's
/// identity does not depend on the `seq` and ids a particular recording carries.
pub(super) fn fold_numbers(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    let mut in_digits = false;
    for character in text.chars() {
        if character.is_ascii_digit() {
            if !in_digits {
                folded.push('#');
            }
            in_digits = true;
        } else {
            folded.push(character);
            in_digits = false;
        }
    }
    folded
}

pub(super) const fn level(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Fail => "error",
        _ => "warning",
    }
}
