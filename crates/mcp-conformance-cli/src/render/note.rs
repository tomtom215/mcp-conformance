// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The note about a server's fixtures: the suite asked for names the server's
//! own listings do not include ([`crate::fixtures`]).

use std::fmt::Write as _;

use crate::run::Outcome;

/// How many scenarios failed ([`mcp_conformance_cli::suite::Scenario::failed`]).
pub(super) fn failed_scenarios(outcome: &Outcome) -> usize {
    outcome
        .scenarios
        .iter()
        .filter(|scenario| scenario.failed())
        .count()
}

/// At most this many names of a kind in the note; the JSON report has them all.
const NAMES_SHOWN: usize = 4;

/// `a, b, c, d and 3 more`.
pub(super) fn some_names(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(NAMES_SHOWN).map(String::as_str).collect();
    match names.len().saturating_sub(NAMES_SHOWN) {
        0 => shown.join(", "),
        more => format!("{} and {more} more", shown.join(", ")),
    }
}

/// Says, when the traffic shows it, that the suite asked a server for names
/// its own listings do not include: the fixtures of the suite's own server. It
/// claims what the requests show and no link to a particular failure, which
/// the recording cannot give ([`crate::fixtures`]).
pub(super) fn fixtures_note(text: &mut String, outcome: &Outcome) {
    let Some(fixtures) = &outcome.fixtures else {
        return;
    };
    let _ = writeln!(
        text,
        "note: {} of the suite's {} scenario(s) failed. In {} of the {} recorded sessions the \
         suite asked for a tool, prompt or resource that none of this server's own list \
         answers in this run includes:",
        failed_scenarios(outcome),
        outcome.scenarios.len(),
        fixtures.sessions,
        fixtures.judged_sessions
    );
    let unlisted = &fixtures.unlisted;
    for (kind, names) in [
        ("tools", &unlisted.tools),
        ("prompts", &unlisted.prompts),
        ("resources", &unlisted.resources),
    ] {
        if !names.is_empty() {
            let _ = writeln!(text, "  {kind} ({}): {}", names.len(), some_names(names));
        }
    }
    let _ = writeln!(
        text,
        "The pinned suite asks for these by fixed names (`test_simple_text` is one), and a \
         server is free to define its own. The recording does not show which failures follow \
         from a missing name: the suite's message for each scenario is above."
    );
}
