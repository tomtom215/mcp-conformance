// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Names the suite asked a server for that the server does not list.
//!
//! The pinned suite's server scenarios call tools, prompts and resources by
//! fixed names (`tools/call test_simple_text`, `prompts/get test_simple_prompt`,
//! `resources/read test://static-text`), so a server that defines its own
//! fixtures fails scenarios for a reason that is not a finding about the
//! server's clauses. A first-time user reads 18 failures as a broken server.
//!
//! This module finds, from the recorded traffic alone, the requests that show
//! it: a `tools/call`, `prompts/get` or `resources/read` (or `subscribe`,
//! `unsubscribe`) for a name that none of the server's own `*/list` answers in
//! the run includes. It claims nothing the traffic does not hold, and so:
//!
//! - a kind with no list answer in the run is not assessed (without the
//!   server's own listing, a request for a name proves nothing);
//! - a kind whose list answer carried a `nextCursor` is not assessed (the
//!   listing may not be whole);
//! - a resource is unlisted only when it matches no listed URI and no listed
//!   template, and a template using anything beyond `{name}` expansions is
//!   taken to match (RFC 6570 operators are not interpreted);
//! - which *scenario* asked is not guessed: the report names requests and the
//!   sessions that hold them, not a link to a particular failure.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde::Serialize;
use serde_json::Value;

/// The names requested that no listing in the run includes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Unlisted {
    /// Tools called (`tools/call`) and not listed (`tools/list`).
    pub(crate) tools: Vec<String>,
    /// Prompts fetched (`prompts/get`) and not listed (`prompts/list`).
    pub(crate) prompts: Vec<String>,
    /// Resource URIs read, subscribed to or unsubscribed from, and neither
    /// listed (`resources/list`) nor matching a listed template
    /// (`resources/templates/list`).
    pub(crate) resources: Vec<String>,
}

impl Unlisted {
    /// How many names there are in all.
    pub(crate) const fn len(&self) -> usize {
        self.tools.len() + self.prompts.len() + self.resources.len()
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What the traffic shows, for the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Fixtures {
    /// The names, by kind.
    pub(crate) unlisted: Unlisted,
    /// Recorded sessions that hold at least one such request.
    pub(crate) sessions: usize,
    /// Recorded sessions looked at.
    pub(crate) judged_sessions: usize,
}

/// One kind of fixture: the request that names it, and the listing that
/// should hold it.
struct Kind {
    /// The methods that name a fixture, and the `params` member holding its name.
    requests: &'static [&'static str],
    member: &'static str,
    list: &'static str,
    /// The result member holding the listed items, and each item's name member.
    items: &'static str,
    item_name: &'static str,
}

const TOOLS: Kind = Kind {
    requests: &["tools/call"],
    member: "name",
    list: "tools/list",
    items: "tools",
    item_name: "name",
};
const PROMPTS: Kind = Kind {
    requests: &["prompts/get"],
    member: "name",
    list: "prompts/list",
    items: "prompts",
    item_name: "name",
};
const RESOURCES: Kind = Kind {
    requests: &[
        "resources/read",
        "resources/subscribe",
        "resources/unsubscribe",
    ],
    member: "uri",
    list: "resources/list",
    items: "resources",
    item_name: "uri",
};
const TEMPLATES: Kind = Kind {
    requests: &[],
    member: "",
    list: "resources/templates/list",
    items: "resourceTemplates",
    item_name: "uriTemplate",
};

/// What one kind's traffic holds across the run.
#[derive(Default)]
struct Seen {
    /// Names requested, and the sessions (by index) that requested each.
    requested: BTreeMap<String, BTreeSet<usize>>,
    /// Names the listings include.
    listed: BTreeSet<String>,
    /// Whether any listing answer was seen.
    answered: bool,
    /// Whether any listing answer said there was more (`nextCursor`).
    paged: bool,
}

/// The unlisted requests in `sessions` (each session's events), or `None`
/// when there are none to report.
pub(crate) fn find(sessions: &[&[TraceEvent]]) -> Option<Fixtures> {
    let mut tools = Seen::default();
    let mut prompts = Seen::default();
    let mut resources = Seen::default();
    let mut templates = Seen::default();
    for (index, events) in sessions.iter().enumerate() {
        scan(
            events,
            index,
            [
                (&TOOLS, &mut tools),
                (&PROMPTS, &mut prompts),
                (&RESOURCES, &mut resources),
                (&TEMPLATES, &mut templates),
            ],
        );
    }
    // Resources are only judged against templates when templates were
    // listed: a server with none lists no template, and that is an answer.
    let simple_templates = parsed_templates(&templates.listed);
    let tools = missing(&tools, |name| !tools.listed.contains(name));
    let prompts = missing(&prompts, |name| !prompts.listed.contains(name));
    let resources = missing(&resources, |uri| {
        !resources.listed.contains(uri) && !served_by_a_template(&simple_templates, uri)
    });
    let unlisted = Unlisted {
        tools: tools.0,
        prompts: prompts.0,
        resources: resources.0,
    };
    if unlisted.is_empty() {
        return None;
    }
    let holding: BTreeSet<usize> = tools
        .1
        .into_iter()
        .chain(prompts.1)
        .chain(resources.1)
        .collect();
    Some(Fixtures {
        unlisted,
        sessions: holding.len(),
        judged_sessions: sessions.len(),
    })
}

/// The requested names `unlisted` says are missing, and the sessions that
/// requested them; nothing for a kind whose listing the run does not hold
/// whole.
fn missing(seen: &Seen, unlisted: impl Fn(&str) -> bool) -> (Vec<String>, BTreeSet<usize>) {
    if !seen.answered || seen.paged {
        return (Vec::new(), BTreeSet::new());
    }
    let mut names = Vec::new();
    let mut sessions = BTreeSet::new();
    for (name, asking) in &seen.requested {
        if unlisted(name) {
            names.push(name.clone());
            sessions.extend(asking);
        }
    }
    (names, sessions)
}

/// Reads one session's messages into the kinds' [`Seen`]s.
fn scan<const N: usize>(events: &[TraceEvent], session: usize, mut kinds: [(&Kind, &mut Seen); N]) {
    // A listing request awaiting its answer: the JSON-RPC id, and which kind.
    let mut pending: BTreeMap<String, usize> = BTreeMap::new();
    for event in events {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        match event.direction {
            Direction::ClientToServer => {
                let Some(method) = payload.get("method").and_then(Value::as_str) else {
                    continue;
                };
                for (index, (kind, seen)) in kinds.iter_mut().enumerate() {
                    if kind.list == method
                        && let Some(id) = payload.get("id")
                    {
                        pending.insert(id.to_string(), index);
                    }
                    if kind.requests.contains(&method)
                        && let Some(name) = payload
                            .get("params")
                            .and_then(|params| params.get(kind.member))
                            .and_then(Value::as_str)
                    {
                        seen.requested
                            .entry(name.to_owned())
                            .or_default()
                            .insert(session);
                    }
                }
            }
            Direction::ServerToClient => {
                let Some(result) = payload.get("result") else {
                    continue;
                };
                let Some(index) = payload
                    .get("id")
                    .and_then(|id| pending.remove(&id.to_string()))
                else {
                    continue;
                };
                let (kind, seen) = &mut kinds[index];
                seen.answered = true;
                seen.paged |= match result.get("nextCursor") {
                    // Absent, null and empty all say there is no more; any
                    // other cursor says there is.
                    None | Some(Value::Null) => false,
                    Some(Value::String(cursor)) => !cursor.is_empty(),
                    Some(_) => true,
                };
                for item in result
                    .get(kind.items)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(name) = item.get(kind.item_name).and_then(Value::as_str) {
                        seen.listed.insert(name.to_owned());
                    }
                }
            }
        }
    }
}

/// A URI template, taken apart: literal text and `{name}` expansions.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Literal(String),
    Variable,
}

/// The listed templates, each either understood (`Some`, its pieces) or not
/// (`None`: it uses an operator or modifier, or is malformed).
fn parsed_templates(listed: &BTreeSet<String>) -> Vec<Option<Vec<Piece>>> {
    listed.iter().map(|template| pieces(template)).collect()
}

/// `template` as pieces, when it is only literal text and plain `{name}`
/// expansions (RFC 6570 level 1); `None` for anything else.
fn pieces(template: &str) -> Option<Vec<Piece>> {
    let mut pieces = Vec::new();
    let mut rest = template;
    while !rest.is_empty() {
        let Some(open) = rest.find(['{', '}']) else {
            pieces.push(Piece::Literal(rest.to_owned()));
            break;
        };
        if rest.as_bytes()[open] == b'}' {
            return None;
        }
        if open > 0 {
            pieces.push(Piece::Literal(rest[..open].to_owned()));
        }
        let close = rest[open..].find('}')? + open;
        let name = &rest[open + 1..close];
        let plain = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '%'));
        if !plain {
            return None;
        }
        pieces.push(Piece::Variable);
        rest = &rest[close + 1..];
    }
    Some(pieces)
}

/// Whether `uri` may be served by one of the templates: it matches an
/// understood one, or some template is not understood (and so might).
fn served_by_a_template(templates: &[Option<Vec<Piece>>], uri: &str) -> bool {
    templates
        .iter()
        .any(|template| template.as_ref().is_none_or(|pieces| matches(pieces, uri)))
}

/// Whether `text` is `pieces` expanded: each variable one or more characters.
fn matches(pieces: &[Piece], text: &str) -> bool {
    match pieces.split_first() {
        None => text.is_empty(),
        Some((Piece::Literal(literal), rest)) => text
            .strip_prefix(literal.as_str())
            .is_some_and(|after| matches(rest, after)),
        Some((Piece::Variable, rest)) => text
            .char_indices()
            .skip(1)
            .map(|(at, _)| at)
            .chain(std::iter::once(text.len()))
            .any(|at| at > 0 && matches(rest, &text[at..])),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
