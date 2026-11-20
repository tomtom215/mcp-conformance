// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The control a refusal is read against: the same request, served.
//!
//! Several transport clauses bind the status of a refusal *for one reason* —
//! "requests without an `MCP-Session-Id`", "an `Origin` header that is present
//! and invalid" — and a status alone never states its reason. A `404` drawn by
//! a request carrying a hostile `Origin` may be the origin's doing, or the
//! path's, or an unknown method's.
//!
//! What a recording can show is the reason by elimination. When the trace holds
//! the refused request and its *twin* — the same HTTP method, the same recorded
//! headers but for the one under judgment, the same JSON-RPC message but for
//! its `id` — and the twin was served, then the one header is the only thing
//! the recording varied, and the refusal is its consequence. That is the
//! stimulus pair a probing client sends, and it is what these checks require
//! before they hold a status against a clause.
//!
//! Two limits follow, both on the safe side. Only [`RECORDED_HEADERS`] are
//! compared, so a difference in a header the recorder drops (`Authorization`,
//! notably) is invisible: [`blames_the_request`] therefore leaves out the
//! statuses such a difference, or the passing of time, explains on its own.
//! And a POST whose body the framing could not pair has no twin, since nothing
//! says what it carried.
//!
//! [`RECORDED_HEADERS`]: mcp_conformance_core::trace::RECORDED_HEADERS

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::stateless::transport::framing::{Exchange, Framing};

/// Whether `status` refuses a request for what the request itself was.
///
/// A `4xx`, less the three that say something else: `401` (a credential the
/// recording never carries), `408` (time) and `429` (rate). A `5xx` is the
/// server's own failure and blames nothing.
pub(super) fn blames_the_request(status: u16) -> bool {
    (400..500).contains(&status) && !matches!(status, 401 | 408 | 429)
}

/// The served twin of `subject`: another exchange in the trace, answered `2xx`,
/// whose request is `subject`'s but for `header` — or exactly `subject`'s, with
/// `header` `None`.
///
/// With a `header`, the two requests must *differ* in it (one may lack it), so
/// an exchange is never its own twin and the header is known to be what varied.
pub(super) fn served_twin<'f, 'a>(
    framing: &'f Framing<'a>,
    subject: &Exchange<'a>,
    header: Option<&str>,
) -> Option<&'f Exchange<'a>> {
    let (_, method, headers) = subject.request?;
    let method = method?;
    framing.exchanges().iter().find(|other| {
        other
            .request
            .is_some_and(|(_, other_method, other_headers)| {
                (200..300).contains(&other.status)
                    && other_method == Some(method)
                    && header.is_none_or(|name| headers.get(name) != other_headers.get(name))
                    && same_but_for(headers, other_headers, header)
                    && same_message(method, subject.carried, other.carried)
            })
    })
}

/// Whether two header maps agree on every header except `header`.
fn same_but_for(
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
    header: Option<&str>,
) -> bool {
    let kept = |(name, _): &(&String, &String)| Some(name.as_str()) != header;
    left.iter().filter(kept).eq(right.iter().filter(kept))
}

/// Whether two requests carried the same JSON-RPC message, ids aside.
///
/// A `GET` or `DELETE` carries none, so two of them agree; any other request
/// whose body the framing left unpaired is not known to agree with anything.
fn same_message(method: &str, left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => without_id(left) == without_id(right),
        (None, None) => matches!(method, "GET" | "DELETE"),
        _ => false,
    }
}

/// `message` with its `id` member removed.
fn without_id(message: &Value) -> Value {
    let mut message = message.clone();
    if let Some(members) = message.as_object_mut() {
        members.remove("id");
    }
    message
}

/// Whether `exchange` carried an `initialize` request.
pub(super) fn carries_initialize(exchange: &Exchange<'_>) -> bool {
    exchange
        .carried
        .and_then(|payload| payload.get("method"))
        .and_then(Value::as_str)
        == Some("initialize")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_a_4xx_with_no_other_cause_blames_the_request() {
        for blaming in [400, 403, 404, 405, 410, 499] {
            assert!(blames_the_request(blaming), "{blaming}");
        }
        for other in [200, 302, 399, 401, 408, 429, 500, 503] {
            assert!(!blames_the_request(other), "{other}");
        }
    }

    #[test]
    fn headers_agree_but_for_the_one_named() {
        let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect()
        };
        let with = map(&[("accept", "a"), ("origin", "x")]);
        let without = map(&[("accept", "a")]);
        let other = map(&[("accept", "b"), ("origin", "x")]);
        assert!(same_but_for(&with, &without, Some("origin")));
        assert!(same_but_for(&with, &with, None));
        assert!(!same_but_for(&with, &without, None));
        assert!(!same_but_for(&with, &without, Some("accept")));
        assert!(!same_but_for(&with, &other, Some("origin")));
    }

    #[test]
    fn messages_agree_ids_aside() {
        let first = json!({"jsonrpc":"2.0","id":1,"method":"ping"});
        let second = json!({"jsonrpc":"2.0","id":2,"method":"ping"});
        let other = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
        assert!(same_message("POST", Some(&first), Some(&second)));
        assert!(!same_message("POST", Some(&first), Some(&other)));
        // A body that is not an object has no id to set aside.
        assert!(same_message("POST", Some(&json!([1])), Some(&json!([1]))));
        assert!(!same_message("POST", Some(&json!([1])), Some(&json!([2]))));
        assert!(same_message("GET", None, None));
        assert!(same_message("DELETE", None, None));
        assert!(!same_message("POST", None, None));
        assert!(!same_message("GET", Some(&first), None));
        assert!(!same_message("GET", None, Some(&first)));
    }
}
