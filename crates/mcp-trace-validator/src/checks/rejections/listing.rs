// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a session's own listings prove about the names a server knows.
//!
//! "The server listed no such prompt" is evidence only when the listing is the
//! whole listing. A paginated enumeration cut short, a recording that carries a
//! later page and not the first, or a `list_changed` notification the client
//! never followed up each leave room for a name the trace does not show, and
//! then nothing may be concluded from a name's absence.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use crate::context::TraceContext;

/// Every item a session's results for one list method carried, and whether
/// they amount to the whole listing.
pub(super) struct Listing<'a> {
    /// The listed items, across every result.
    items: Vec<&'a Value>,
    /// Whether the results form a complete enumeration: a first page was
    /// requested, and every `nextCursor` a page issued was followed to a page
    /// of its own.
    complete: bool,
    /// Whether the server announced that the list changed.
    changed: bool,
}

impl<'a> Listing<'a> {
    /// Reads the results of `method`, whose items sit under `key`, and whether
    /// the server sent the `changed` notification.
    pub(super) fn of(
        context: &TraceContext<'a>,
        method: &'static str,
        key: &str,
        changed: &str,
    ) -> Self {
        let mut items = Vec::new();
        let mut first_page = false;
        let mut malformed = false;
        let mut issued: BTreeSet<&str> = BTreeSet::new();
        let mut followed: BTreeSet<&str> = BTreeSet::new();
        for exchange in context.exchanges_for(method) {
            let Some(page) = exchange
                .result
                .and_then(|result| result.get(key))
                .and_then(Value::as_array)
            else {
                continue;
            };
            items.extend(page);
            match exchange.params.and_then(|params| params.get("cursor")) {
                None | Some(Value::Null) => first_page = true,
                Some(cursor) => followed.extend(cursor.as_str()),
            }
            match exchange.result.and_then(|result| result.get("nextCursor")) {
                None | Some(Value::Null) => {}
                Some(Value::String(cursor)) => {
                    issued.insert(cursor);
                }
                // Not a cursor anyone could follow, but not "no further page"
                // either: the enumeration cannot be shown whole.
                Some(_) => malformed = true,
            }
        }
        let changed = context.messages().any(|(event, kind, _)| {
            event.direction == Direction::ServerToClient
                && matches!(kind, MessageKind::Notification { method } if *method == changed)
        });
        Self {
            items,
            complete: first_page && !malformed && issued.is_subset(&followed),
            changed,
        }
    }

    /// Whether the session proves the listing whole and unchanged, so that a
    /// name absent from it is a name the server did not list.
    pub(super) const fn is_exhaustive(&self) -> bool {
        self.complete && !self.changed
    }

    /// The string each listed item carries under `field`.
    pub(super) fn strings(&self, field: &str) -> BTreeSet<&'a str> {
        self.items
            .iter()
            .filter_map(|item| item.get(field).and_then(Value::as_str))
            .collect()
    }

    /// The names the server listed, when the listing is whole and unchanged;
    /// `None` when a name's absence would prove nothing.
    pub(super) fn known(&self, field: &str) -> Option<BTreeSet<&'a str>> {
        self.is_exhaustive().then(|| self.strings(field))
    }

    /// For each listed prompt, the arguments *every* listing of it marked
    /// `required: true`.
    ///
    /// The intersection, so that a prompt the server re-declared with fewer
    /// required arguments is held only to what it never stopped requiring; and
    /// empty once the server announced a change the recording may not have
    /// followed.
    pub(super) fn required_arguments(&self) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
        let mut required: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        if self.changed {
            return required;
        }
        for item in &self.items {
            let Some(name) = item.get("name").and_then(Value::as_str) else {
                continue;
            };
            let declared: BTreeSet<&'a str> = item
                .get("arguments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|argument| argument.get("required").and_then(Value::as_bool) == Some(true))
                .filter_map(|argument| argument.get("name").and_then(Value::as_str))
                .collect();
            required
                .entry(name)
                .and_modify(|held| held.retain(|argument| declared.contains(argument)))
                .or_insert(declared);
        }
        required
    }
}
