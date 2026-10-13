// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the authorization pages (`AUTH-*`): what a protected MCP endpoint's
//! challenges must say, at both revisions.
//!
//! **What a recording carries for authorization, and what it does not.** Over
//! Streamable HTTP a trace holds each exchange's method, status and allowlisted
//! headers (`RECORDED_HEADERS`), and its JSON body. Since 2026-10-13 the list
//! includes `WWW-Authenticate`, the server's challenge, which carries no
//! credential. Everything else on these pages is out of a recording's reach by
//! construction, and the registry's exclusions say which reason applies:
//!
//! - **The client's credential.** `Authorization` is never recorded, so neither
//!   "authorization in every request" nor which token was sent can be judged.
//! - **The request target.** An `http` event records no path or query, so a
//!   token in the query string cannot be seen, and a Protected Resource
//!   Metadata document cannot be told from any other JSON body.
//! - **Everything off the MCP endpoint.** The `resource_metadata` URL names the
//!   server's own address, which a capture proxy does not rewrite, so a client
//!   fetches the metadata past the proxy; the well-known fallback is built from
//!   the proxy's origin and mapped under the upstream's path, where a server
//!   serves no metadata. Authorization-server metadata, registration, PKCE,
//!   the authorization and token requests and consent all happen between the
//!   client, the browser and the authorization server — never on this wire.
//! - **The server's validation.** Whether a token was valid, for this
//!   audience, is decided inside the server; the challenge is what it says
//!   about the decision, and that is what is judged here.
//!
//! So the judged subset is the challenge: its `scope`, its `error` code
//! against the status, the `insufficient_scope` response's parameters, and
//! (`2026-07-28`) the scopes it never asks for and asks for at once. Each check
//! reads the parsed challenge on a server response's `WWW-Authenticate`; a
//! response recorded before the header was allowlisted carries none, so these
//! clauses read *not observed* on it rather than failing.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::trace::{Direction, EventBody};

use super::FindingSink;
use super::stateless::transport::framing::Framing;
use crate::context::TraceContext;

mod challenge;
#[cfg(test)]
mod tests;

use challenge::Challenge;

/// Every server response that carries a `WWW-Authenticate` header: its `seq`,
/// status, and parsed challenges.
fn challenges<'a>(
    context: &'a TraceContext<'_>,
) -> impl Iterator<Item = (u64, Option<u16>, Vec<Challenge>)> + 'a {
    context
        .events()
        .iter()
        .filter(|event| event.direction == Direction::ServerToClient)
        .filter_map(|event| match &event.body {
            EventBody::Http {
                status, headers, ..
            } => headers
                .get("www-authenticate")
                .map(|value| (event.seq, *status, challenge::parse(value))),
            _ => None,
        })
}

/// The Bearer challenge among `challenges`, when there is one.
fn bearer(challenges: &[Challenge]) -> Option<&Challenge> {
    challenges.iter().find(|challenge| challenge.is_bearer())
}

/// The challenge declaring `error="insufficient_scope"`, whatever its scheme.
fn insufficient_scope(challenges: &[Challenge]) -> Option<&Challenge> {
    challenges
        .iter()
        .find(|challenge| challenge.param("error") == Some("insufficient_scope"))
}

/// A `scope` parameter's scope tokens (RFC 6749 §3.3: space-delimited).
fn scopes(challenge: &Challenge) -> BTreeSet<&str> {
    challenge
        .param("scope")
        .map(|scope| scope.split_ascii_whitespace().collect())
        .unwrap_or_default()
}

fn status_text(status: Option<u16>) -> String {
    status.map_or_else(
        || "a response with no status".to_owned(),
        |code| format!("HTTP {code}"),
    )
}

/// `AUTH-014` / `AUTH-097`: a `401`'s Bearer challenge names, in `scope`, the
/// scopes the resource needs. The subject is a `401` carrying one; a `401`
/// with no challenge at all has no header for the parameter to be in.
pub(super) fn challenge_scope(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, status, challenges) in challenges(context) {
        if status != Some(401) {
            continue;
        }
        let Some(challenge) = bearer(&challenges) else {
            continue;
        };
        sink.examined();
        if challenge.param("scope").is_none() {
            sink.push(
                Some(seq),
                "HTTP 401's Bearer challenge carries no `scope` parameter; servers should name \
                 the scopes required for the resource (RFC 6750 §3)"
                    .to_owned(),
            );
        }
    }
}

/// `AUTH-050` / `AUTH-131`: the insufficient-scope response is a `403` with a
/// Bearer challenge carrying `scope` and `resource_metadata`. The subject is a response whose challenge says
/// `error="insufficient_scope"` — the server's own statement of the
/// antecedent, which is why it can be judged at all.
pub(super) fn insufficient_scope_response(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, status, challenges) in challenges(context) {
        let Some(challenge) = insufficient_scope(&challenges) else {
            continue;
        };
        sink.examined();
        let mut wrong = Vec::new();
        if status != Some(403) {
            wrong.push(format!(
                "came with {}, not 403 Forbidden",
                status_text(status)
            ));
        }
        if !challenge.is_bearer() {
            wrong.push(format!(
                "uses the {:?} scheme, not Bearer",
                challenge.scheme
            ));
        }
        for parameter in ["scope", "resource_metadata"] {
            if challenge.param(parameter).is_none() {
                wrong.push(format!("has no `{parameter}` parameter"));
            }
        }
        if !wrong.is_empty() {
            sink.push(
                Some(seq),
                format!(
                    "an insufficient_scope challenge {}; the server should answer with 403, a \
                     Bearer challenge, `scope` and `resource_metadata`",
                    wrong.join(", ")
                ),
            );
        }
    }
}

/// `AUTH-051` / `AUTH-132`: an insufficient-scope challenge names the scopes
/// the request needs. Which scopes those are is the server's to know; that
/// some are named is what a recording can hold it to.
pub(super) fn insufficient_scope_names_scopes(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, _, challenges) in challenges(context) {
        let Some(challenge) = insufficient_scope(&challenges) else {
            continue;
        };
        sink.examined();
        if scopes(challenge).is_empty() {
            sink.push(
                Some(seq),
                "an insufficient_scope challenge names no scope; servers should include the \
                 scopes needed to satisfy the request in `scope`"
                    .to_owned(),
            );
        }
    }
}

/// The status the error-handling table gives each Bearer error code (it names
/// the codes RFC 6750 §3.1 defines, by meaning).
const fn table_status(error: &str) -> Option<u16> {
    match error.as_bytes() {
        b"invalid_request" => Some(400),
        b"invalid_token" => Some(401),
        b"insufficient_scope" => Some(403),
        _ => None,
    }
}

/// `AUTH-049` / `AUTH-130`: the status matches the error the server's own
/// challenge declares — `400` for a malformed authorization request, `401` for an
/// invalid token, `403` for insufficient scope. A challenge with no `error`
/// (authorization required, no token sent) and an error code the table does
/// not name are not subjects.
pub(super) fn error_status(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, status, challenges) in challenges(context) {
        for challenge in challenges.iter().filter(|challenge| challenge.is_bearer()) {
            let Some(error) = challenge.param("error") else {
                continue;
            };
            let Some(expected) = table_status(error) else {
                continue;
            };
            sink.examined();
            if status != Some(expected) {
                sink.push(
                    Some(seq),
                    format!(
                        "{} carries a Bearer challenge with error={error:?}; that authorization \
                         error is answered with HTTP {expected}",
                        status_text(status)
                    ),
                );
            }
        }
    }
}

/// `AUTH-045` / `AUTH-124`: invalid or expired tokens receive `401`. The subject is a response whose
/// Bearer challenge says `error="invalid_token"`, the server's statement that
/// the token failed validation.
pub(super) fn invalid_token_401(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, status, challenges) in challenges(context) {
        let declares = challenges.iter().any(|challenge| {
            challenge.is_bearer() && challenge.param("error") == Some("invalid_token")
        });
        if !declares {
            continue;
        }
        sink.examined();
        if status != Some(401) {
            sink.push(
                Some(seq),
                format!(
                    "{} refuses a token the challenge calls invalid_token; invalid or expired \
                     tokens must receive HTTP 401",
                    status_text(status)
                ),
            );
        }
    }
}

/// `AUTH-129` (`2026-07-28`): servers should not put `offline_access` in a
/// challenge's `scope`. The sentence also covers Protected Resource Metadata's
/// `scopes_supported`, which a recording cannot identify (no request target is
/// recorded); this judges the challenge, the half on the MCP endpoint.
pub(super) fn no_offline_access_challenge(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, _, challenges) in challenges(context) {
        for challenge in challenges
            .iter()
            .filter(|challenge| challenge.param("scope").is_some())
        {
            sink.examined();
            if scopes(challenge).contains("offline_access") {
                sink.push(
                    Some(seq),
                    "a WWW-Authenticate challenge's `scope` includes offline_access; refresh \
                     tokens are not a resource requirement"
                        .to_owned(),
                );
            }
        }
    }
}

/// `AUTH-133` (`2026-07-28`): all the scopes an operation needs in a single
/// challenge. The
/// subject is a retry — the same request, sent again after an
/// insufficient-scope challenge refused it; challenging that retry for a scope
/// the earlier challenge did not name is the incremental challenging the
/// clause forbids. A retry is recognised by its message, which a client sends
/// again unchanged; only an exchange whose body the framing pairs with
/// certainty is read.
pub(super) fn single_challenge(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // Request message (canonical text) → every scope its challenges named.
    let mut challenged: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for exchange in Framing::new(context).exchanges() {
        let Some(carried) = exchange.carried else {
            continue;
        };
        let key = to_canonical_string(carried);
        let current = exchange
            .headers
            .get("www-authenticate")
            .map(|value| challenge::parse(value))
            .unwrap_or_default();
        let named: Option<BTreeSet<String>> = insufficient_scope(&current)
            .map(|challenge| scopes(challenge).into_iter().map(str::to_owned).collect());
        if let Some(earlier) = challenged.get(&key) {
            sink.examined();
            if let Some(named) = &named {
                let new: Vec<&str> = named.difference(earlier).map(String::as_str).collect();
                if !new.is_empty() {
                    sink.push(
                        Some(exchange.status_seq),
                        format!(
                            "a retried request is challenged again for {} that the earlier \
                             insufficient_scope challenge did not name; servers should include \
                             every scope the operation needs in a single challenge",
                            new.join(" ")
                        ),
                    );
                }
            }
        }
        if let Some(named) = named {
            challenged.entry(key).or_default().extend(named);
        }
    }
}
