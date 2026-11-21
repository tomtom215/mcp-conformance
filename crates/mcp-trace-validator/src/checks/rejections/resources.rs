// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! server/resources#error-handling: the code a refused `resources/read`
//! carries, which the two revisions assign differently.
//!
//! Whether a resource exists is the server's knowledge, and a server may serve
//! a resource it lists nowhere — templates exist precisely because not every
//! readable URI is enumerable. So a *result* is never judged here. What is
//! judged is a **refusal of a URI whose scheme the server lists nowhere**: the
//! session holds a complete, unchanged `resources/list` and a complete
//! `resources/templates/list`, and neither names a single URI or template under
//! the requested URI's scheme. Refusing such a URI, the server can only be
//! saying it has no such resource, and the clause names the code for that.
//!
//! The scheme, not the whole URI, is the unit on purpose. Matching a URI
//! against RFC 6570 templates to prove it matches none is a judgment a
//! validator can get wrong in the server's disfavour; comparing schemes cannot
//! be, and it also keeps clear of the one refusal a listing cannot explain —
//! access denied to an unlisted resource *beside* the listed ones.

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::FindingSink;
use super::super::support::has_rfc3986_scheme;
use super::listing::Listing;
use super::{Answer, INVALID_PARAMS};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The code `2025-11-25` assigns to a missing resource, withdrawn at
/// `2026-07-28`.
const RESOURCE_NOT_FOUND: i64 = -32002;

/// The list-changed notification both resource listings answer to.
const LIST_CHANGED: &str = "notifications/resources/list_changed";

/// The lower-cased scheme of `uri`, when it has one (RFC 3986 §3.1: schemes
/// compare case-insensitively).
fn scheme(uri: &str) -> Option<String> {
    has_rfc3986_scheme(uri)
        .then(|| uri.split_once(':'))
        .flatten()
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
}

/// Every scheme the server listed a resource or template under, or `None` when
/// the session does not prove that set whole: a listing is partial, missing or
/// announced as changed, or a template leaves its scheme to a variable.
fn listed_schemes(context: &TraceContext<'_>) -> Option<BTreeSet<String>> {
    let resources = Listing::of(context, "resources/list", "resources", LIST_CHANGED);
    let templates = Listing::of(
        context,
        "resources/templates/list",
        "resourceTemplates",
        LIST_CHANGED,
    );
    if !resources.is_exhaustive() || !templates.is_exhaustive() {
        return None;
    }
    let mut schemes: BTreeSet<String> = resources
        .strings("uri")
        .into_iter()
        .filter_map(scheme)
        .collect();
    for template in templates.strings("uriTemplate") {
        // A template whose scheme is itself an expression (`{scheme}://…`), or
        // that has none, may expand to any scheme at all.
        schemes.insert(scheme(template)?);
    }
    Some(schemes)
}

/// Every `resources/read` the server *refused* for a URI under a scheme it
/// lists nowhere, with the URI and the refusal — `-32603` aside, which neither
/// revision's clause lets a trace judge.
fn refused_unlisted<'a>(
    context: &TraceContext<'a>,
) -> Vec<(crate::context::Exchange<'a>, &'a str, Answer<'a>)> {
    let Some(schemes) = listed_schemes(context) else {
        return Vec::new();
    };
    context
        .exchanges_for("resources/read")
        .filter_map(|exchange| {
            let uri = exchange.params?.get("uri").and_then(Value::as_str)?;
            let answer = Answer::of(&exchange);
            let unlisted = scheme(uri).is_some_and(|scheme| !schemes.contains(&scheme));
            (unlisted && matches!(answer, Answer::Error(_)) && !answer.is_another_failure())
                .then_some((exchange, uri, answer))
        })
        .collect()
}

/// `RES-008` (`2025-11-25`): "Resource not found: `-32002`".
///
/// **Stimulus:** a `resources/read` of a URI under a scheme the session's
/// complete listings name nowhere (see the module), answered with an error.
///
/// **Conforming answer:** `-32002`.
///
/// **Not judged:** `-32603` (the clause's other case), and `-32602` — a server
/// that reads an unsupported scheme as an *invalid* parameter rather than a
/// missing resource has a defensible reading of its own URI space, and the
/// clause says nothing against it.
///
/// **Reported:** any other code. The failure case the server is in is one the
/// page names, and the code is neither of the standard ones it asks for.
pub(in crate::checks) fn not_found_code(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (exchange, uri, answer) in refused_unlisted(context) {
        if answer.is_error(INVALID_PARAMS) {
            continue;
        }
        sink.examined();
        if !answer.is_error(RESOURCE_NOT_FOUND) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`resources/read` of {uri:?}, whose scheme the session's complete \
                     listings name nowhere, was refused with {} rather than -32002",
                    answer.describe()
                ),
            );
        }
    }
}

/// `RES-019` (`2026-07-28`): "If the requested resource does not exist,
/// servers MUST return a JSON-RPC error with code `-32602`".
///
/// **Violation (fail):** a `resources/read` answered `-32002`. That code
/// means one thing — it is the "resource not found" of earlier revisions,
/// which this page still asks clients to accept as such — so the answer is
/// the server's own statement that the resource does not exist, in the wrong
/// code. It is the only witness of a violation a recording holds.
///
/// **Conforming answer (pass):** `-32602` to a `resources/read` of a URI
/// under a scheme the session's complete listings name nowhere (see the
/// module): the server refused a resource it does not have with the code the
/// clause names.
///
/// **Not judged:** any result (the server's word that the resource exists);
/// `-32603`, which the next sentence of the page assigns to internal errors;
/// and any other code for a URI under an unlisted scheme. That a refusal of
/// such a URI means "does not exist" is this tool's inference, not the
/// server's statement — it may be refusing access, or the request's shape —
/// and a MUST is not failed on an inference (narrowed 2026-11-21; such an
/// answer was reported before).
pub(in crate::checks) fn missing_resource_code(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let refused: BTreeSet<u64> = refused_unlisted(context)
        .into_iter()
        .map(|(exchange, _, _)| exchange.response.seq)
        .collect();
    for exchange in context.exchanges_for("resources/read") {
        let answer = Answer::of(&exchange);
        let withdrawn = answer.is_error(RESOURCE_NOT_FOUND);
        let conforming =
            answer.is_error(INVALID_PARAMS) && refused.contains(&exchange.response.seq);
        if !withdrawn && !conforming {
            continue;
        }
        sink.examined();
        if !withdrawn {
            continue;
        }
        let uri = exchange
            .params
            .and_then(|params| params.get("uri"))
            .map_or_else(|| "null".to_owned(), ToString::to_string);
        sink.push(
            Some(exchange.response.seq),
            format!(
                "`resources/read` of {uri} was refused with {}, the code earlier revisions \
                 used for a resource that does not exist; a missing resource must draw -32602",
                answer.describe()
            ),
        );
    }
}
