// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Protected Resource Metadata (RFC 9728): the document that tells a client
//! where the authorization servers are.
//!
//! The document is not on the MCP endpoint, so a session's recording never
//! holds it. A recording holds it when something fetched it through the
//! recorder: `mcp-conformance check` does, for a server that answered `401`,
//! and the recorder then keeps the request's path (it is under
//! `/.well-known/`, where only public metadata lives) and the JSON document
//! that came back.
//!
//! **What a path can and cannot say.** A fetch is recorded with its path and
//! no host. It is tied to a challenge's `resource_metadata` URL by that
//! URL's path alone, on the premise that a recording is of one server. A
//! recording assembled from two servers that serve different documents at
//! one path would falsify a judgement made here.

use serde_json::Value;

use super::{bearer, challenges};
use crate::checks::FindingSink;
use crate::context::TraceContext;
use crate::fetches::{Answer, Fetch};

/// Where RFC 9728 §3 puts the document: this path, alone or followed by the
/// resource's own path.
const WELL_KNOWN: &str = "/.well-known/oauth-protected-resource";

/// The path of the absolute URL `url`, without query or fragment; `None`
/// when `url` is not an absolute `http(s)` URL.
fn url_path(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let (authority, path) = rest
        .find('/')
        .map_or((rest, "/"), |slash| rest.split_at(slash));
    (!authority.is_empty()).then_some(path)
}

/// The paths the trace's `Bearer` challenges name as `resource_metadata`.
fn named_paths(context: &TraceContext<'_>) -> Vec<String> {
    let mut paths: Vec<String> = challenges(context)
        .filter_map(|(_, _, challenges)| {
            let url = bearer(&challenges)?.param("resource_metadata")?;
            url_path(url).map(str::to_owned)
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Whether `status` says the server could not answer, as opposed to how it
/// answers: a server error, a timeout, a rate limit, or a redirect to
/// somewhere the recording does not follow.
const fn unanswered(status: u16) -> bool {
    status >= 500 || status < 200 || matches!(status, 300..=399 | 408 | 429)
}

/// The fetches of `path` that were answered one way or the other.
fn answered<'f, 'a>(
    fetches: &'f [Fetch<'a>],
    path: &'f str,
) -> impl Iterator<Item = Answer<'a>> + 'f {
    fetches
        .iter()
        .filter(move |fetch| fetch.path == path)
        .filter_map(|fetch| fetch.answer)
        .filter(|answer| !unanswered(answer.status))
}

/// `AUTH-006`, `AUTH-010` and their `2026-07-28` counterparts `AUTH-091`,
/// `AUTH-141`: a server implements Protected Resource Metadata.
///
/// **Stimulus:** a fetch of the path a `Bearer` challenge in the same trace
/// names as its `resource_metadata`, answered with a `2xx` or a `4xx` other
/// than `408` and `429`.
///
/// **Conforming answer:** `200` carrying a JSON object. The server said
/// where its metadata is, and it is there.
///
/// **Violation (fail):** any other answer. The server pointed its clients at
/// a URL that does not serve the document, so they cannot find its
/// authorization servers.
///
/// **Not judged here:** what the document says
/// ([`resource_metadata_names_servers`]); a server that names no URL and
/// relies on the well-known location, whose absence a recording cannot show.
pub(in crate::checks) fn resource_metadata_served(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for path in named_paths(context) {
        for answer in answered(context.fetches(), &path) {
            sink.examined();
            let served = answer.status == 200 && answer.document.is_some_and(Value::is_object);
            if served {
                continue;
            }
            let got = if answer.status == 200 {
                "HTTP 200 without a JSON object".to_owned()
            } else {
                format!("HTTP {}", answer.status)
            };
            sink.push(
                Some(answer.seq),
                format!(
                    "the server's challenge names `{path}` as its Protected Resource Metadata, \
                     and a request for it was answered {got}; the document a client needs to \
                     find the authorization servers is not served there"
                ),
            );
        }
    }
}

/// `AUTH-011` / `AUTH-142`: "The Protected Resource Metadata document
/// returned by the MCP server MUST include the `authorization_servers` field
/// containing at least one authorization server."
///
/// **Stimulus:** a fetch of the well-known Protected Resource Metadata path
/// (alone, or followed by a resource's path) answered `200` with a JSON
/// object: a document the server returned.
///
/// **Conforming answer:** `authorization_servers` is an array holding at
/// least one non-empty string.
///
/// **Violation (fail):** the member is absent, is not an array, is empty, or
/// holds something that is not a non-empty string.
pub(in crate::checks) fn resource_metadata_names_servers(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let documents = context
        .fetches()
        .iter()
        .filter(|fetch| {
            fetch
                .path
                .strip_prefix(WELL_KNOWN)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
        .filter_map(|fetch| fetch.answer)
        .filter(|answer| answer.status == 200)
        .filter_map(|answer| Some((answer.seq, answer.document?.as_object()?)));
    for (seq, document) in documents {
        sink.examined();
        let problem = match document.get("authorization_servers") {
            None => "has no `authorization_servers` member",
            Some(Value::Array(servers)) if servers.is_empty() => {
                "has an empty `authorization_servers`"
            }
            Some(Value::Array(servers))
                if servers
                    .iter()
                    .all(|server| server.as_str().is_some_and(|server| !server.is_empty())) =>
            {
                continue;
            }
            Some(Value::Array(_)) => {
                "has an `authorization_servers` entry that is not a non-empty string"
            }
            Some(_) => "has an `authorization_servers` that is not an array",
        };
        sink.push(
            Some(seq),
            format!(
                "the Protected Resource Metadata document the server returned {problem}; it \
                 must name at least one authorization server"
            ),
        );
    }
}

#[cfg(test)]
mod tests;
