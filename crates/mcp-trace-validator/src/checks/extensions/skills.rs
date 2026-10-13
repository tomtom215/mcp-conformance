// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The skills extension (`io.modelcontextprotocol/skills`, specified in
//! `modelcontextprotocol/ext-skills`, `specification/stable/skills.mdx`, against
//! base revision `2026-07-28`): a server lists skills with `skills/list`, returns
//! one with `skills/get`, serves their files as resources, and may list a
//! directory with `resources/directory/read`.
//!
//! This module judges the serving side — the methods, their results and errors,
//! and directory reads. [`entries`] judges what a skill entry says about its
//! skill. Nearly everything else on the page binds the host's own handling of
//! skill content, which never reaches the wire; the registry says why, clause by
//! clause.

use std::collections::BTreeSet;

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use super::error_code;
use crate::context::{Exchange, TraceContext};

pub(in crate::checks) mod content;
pub(in crate::checks) mod entries;

#[cfg(test)]
mod tests;

/// The extension's identifier, as capabilities name it.
const SKILLS: &str = "io.modelcontextprotocol/skills";

/// The optional method `directoryRead` declares.
const DIRECTORY_READ: &str = "resources/directory/read";

/// `Method not found`.
const METHOD_NOT_FOUND: i64 = -32601;

/// `Invalid params`, the code for an unknown skill or a non-directory.
const INVALID_PARAMS: i64 = -32602;

/// `Internal error`.
const INTERNAL_ERROR: i64 = -32603;

/// What the server's `server/discover` result says about the extension.
#[derive(Debug, Clone, Copy)]
enum Declaration<'a> {
    /// The trace carries no discover result to read a declaration from.
    Unknowable,
    /// The server declares no such extension.
    Withheld,
    /// The server declares it, with these settings.
    Declared(&'a Value),
}

fn declaration<'a>(context: &'a TraceContext<'a>) -> Declaration<'a> {
    let Some(result) = context
        .exchanges_for("server/discover")
        .find_map(|exchange| exchange.result)
    else {
        return Declaration::Unknowable;
    };
    result
        .get("capabilities")
        .and_then(|capabilities| capabilities.get("extensions")?.get(SKILLS))
        .filter(|settings| !settings.is_null() && settings.as_bool() != Some(false))
        .map_or(Declaration::Withheld, Declaration::Declared)
}

/// Whether the declaration's settings set `directoryRead: true`.
fn directory_read(settings: &Value) -> bool {
    settings.get("directoryRead").and_then(Value::as_bool) == Some(true)
}

/// The URI a request names in `params.uri`.
fn uri_param<'a>(exchange: &Exchange<'a>) -> Option<&'a str> {
    exchange.params?.get("uri")?.as_str()
}

/// `SKILL-002`, `SKILL-044`: a server declaring the extension answers
/// `skills/list` — anything but `Method not found`.
pub(in crate::checks) fn list_implemented(context: &TraceContext<'_>, sink: &mut FindingSink) {
    implemented(context, sink, "skills/list", |_| true);
}

/// `SKILL-002`, `SKILL-048`: a server declaring the extension answers `skills/get`.
pub(in crate::checks) fn get_implemented(context: &TraceContext<'_>, sink: &mut FindingSink) {
    implemented(context, sink, "skills/get", |_| true);
}

/// `SKILL-003`: a server declaring `directoryRead: true` answers
/// `resources/directory/read`.
pub(in crate::checks) fn directory_read_implemented(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    implemented(context, sink, DIRECTORY_READ, directory_read);
}

/// Every `method` call to a server whose declaration satisfies `declared` is
/// answered with something other than `Method not found`.
fn implemented(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    method: &str,
    declared: fn(&Value) -> bool,
) {
    let Declaration::Declared(settings) = declaration(context) else {
        return;
    };
    if !declared(settings) {
        return;
    }
    for exchange in context
        .exchanges()
        .filter(|exchange| exchange.method == method)
    {
        sink.examined();
        if error_code(exchange.response) == Some(METHOD_NOT_FOUND) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the server declares `{SKILLS}` but answered `{method}` with `Method not \
                     found`"
                ),
            );
        }
    }
}

/// `SKILL-004`: a client calls `resources/directory/read` only on a server that
/// declared `directoryRead: true`. Judged where the trace carries the server's
/// `server/discover` result; without one the declaration is unknowable.
pub(in crate::checks) fn directory_read_declared(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let allowed = match declaration(context) {
        Declaration::Unknowable => return,
        Declaration::Withheld => false,
        Declaration::Declared(settings) => directory_read(settings),
    };
    for (event, kind, _) in context.messages() {
        let MessageKind::Request { method, .. } = kind else {
            continue;
        };
        if *method != DIRECTORY_READ || event.direction != Direction::ClientToServer {
            continue;
        }
        sink.examined();
        if !allowed {
            sink.push(
                Some(event.seq),
                format!(
                    "the client called `{DIRECTORY_READ}` though the server's `server/discover` \
                     result does not declare `directoryRead: true`"
                ),
            );
        }
    }
}

/// `SKILL-005`: a server declaring the extension declares `resources` too.
pub(in crate::checks) fn declares_resources(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Declaration::Declared(_) = declaration(context) else {
        return;
    };
    let Some(exchange) = context
        .exchanges_for("server/discover")
        .find(|exchange| exchange.result.is_some())
    else {
        return;
    };
    sink.examined();
    let resources = exchange
        .result
        .and_then(|result| result.get("capabilities")?.get("resources"));
    if resources.is_none_or(|value| value.is_null() || value.as_bool() == Some(false)) {
        sink.push(
            Some(exchange.response.seq),
            format!("the server declares `{SKILLS}` without the `resources` capability"),
        );
    }
}

/// `SKILL-045`: `skills/list` results are `resultType: "complete"`.
pub(in crate::checks) fn list_result_complete(context: &TraceContext<'_>, sink: &mut FindingSink) {
    complete(context, sink, "skills/list");
}

/// `SKILL-049`: `skills/get` results are `resultType: "complete"`.
pub(in crate::checks) fn get_result_complete(context: &TraceContext<'_>, sink: &mut FindingSink) {
    complete(context, sink, "skills/get");
}

/// `SKILL-065`: `resources/directory/read` results are `resultType: "complete"`.
pub(in crate::checks) fn directory_result_complete(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    complete(context, sink, DIRECTORY_READ);
}

fn complete(context: &TraceContext<'_>, sink: &mut FindingSink, method: &str) {
    for exchange in context
        .exchanges()
        .filter(|exchange| exchange.method == method)
    {
        let Some(result) = exchange.result else {
            continue;
        };
        sink.examined();
        if result.get("resultType").and_then(Value::as_str) != Some("complete") {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the `{method}` result's `resultType` is {}, not \"complete\"",
                    result
                        .get("resultType")
                        .map_or_else(|| "absent".to_owned(), ToString::to_string)
                ),
            );
        }
    }
}

/// `SKILL-047`: `skills/list` results carry `ttlMs` and `cacheScope`.
pub(in crate::checks) fn list_cacheable(context: &TraceContext<'_>, sink: &mut FindingSink) {
    cacheable(context, sink, "skills/list");
}

/// `SKILL-053`: `skills/get` results carry `ttlMs` and `cacheScope`.
pub(in crate::checks) fn get_cacheable(context: &TraceContext<'_>, sink: &mut FindingSink) {
    cacheable(context, sink, "skills/get");
}

fn cacheable(context: &TraceContext<'_>, sink: &mut FindingSink, method: &str) {
    for exchange in context
        .exchanges()
        .filter(|exchange| exchange.method == method)
    {
        let Some(result) = exchange.result else {
            continue;
        };
        sink.examined();
        let missing: Vec<&str> = ["ttlMs", "cacheScope"]
            .into_iter()
            .filter(|key| result.get(key).is_none())
            .collect();
        if !missing.is_empty() {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the `{method}` result lacks {}, which a cacheable result requires",
                    missing
                        .iter()
                        .map(|key| format!("`{key}`"))
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
            );
        }
    }
}

/// `SKILL-050`: a client's `skills/get` names the URI of a skill's `SKILL.md`.
pub(in crate::checks) fn get_uri_names_skill_md(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, kind, _) in context.messages() {
        let MessageKind::Request { method, .. } = kind else {
            continue;
        };
        if *method != "skills/get" || event.direction != Direction::ClientToServer {
            continue;
        }
        sink.examined();
        let uri = event
            .message_payload()
            .and_then(|payload| payload.get("params")?.get("uri")?.as_str());
        if !uri.is_some_and(entries::is_skill_md) {
            sink.push(
                Some(event.seq),
                format!(
                    "`skills/get` names {}, not the URI of a skill's `SKILL.md`",
                    uri.map_or_else(|| "no `uri`".to_owned(), |uri| format!("{uri:?}"))
                ),
            );
        }
    }
}

/// `SKILL-051`, `SKILL-071`: `skills/get` for a URI that cannot name a skill —
/// one that is not a `SKILL.md` — is refused with `Invalid params`, and any
/// error `skills/get` draws is one of the codes the page assigns.
///
/// `Method not found` is left to `SKILL-002`: a server answering it has not
/// implemented the method, which that clause judges.
pub(in crate::checks) fn get_unknown_invalid_params(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges_for("skills/get") {
        let code = error_code(exchange.response);
        let not_a_skill = !uri_param(&exchange).is_some_and(entries::is_skill_md);
        if !not_a_skill && code.is_none_or(|code| code == METHOD_NOT_FOUND) {
            continue;
        }
        sink.examined();
        if not_a_skill && code != Some(INVALID_PARAMS) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`skills/get` for {:?}, which is no skill's `SKILL.md`, was not refused with \
                     `{INVALID_PARAMS}`",
                    uri_param(&exchange).unwrap_or_default()
                ),
            );
        } else if let Some(code) =
            code.filter(|code| ![INVALID_PARAMS, INTERNAL_ERROR, METHOD_NOT_FOUND].contains(code))
        {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`skills/get` was refused with `{code}`; the codes the extension assigns are \
                     `{INVALID_PARAMS}` (unknown skill) and `{INTERNAL_ERROR}` (internal error)"
                ),
            );
        }
    }
}

/// `SKILL-066`, `SKILL-071`: `resources/directory/read` of a URI the trace shows
/// to be a file is refused with `Invalid params`, and any error the method draws
/// is one of the codes the page assigns.
pub(in crate::checks) fn directory_read_invalid_params(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let files = entries::known_files(context);
    for exchange in context.exchanges_for(DIRECTORY_READ) {
        let code = error_code(exchange.response);
        let a_file = uri_param(&exchange).is_some_and(|uri| files.contains(uri));
        if !a_file && code.is_none_or(|code| code == METHOD_NOT_FOUND) {
            continue;
        }
        sink.examined();
        if a_file && code != Some(INVALID_PARAMS) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`{DIRECTORY_READ}` of {:?}, a file this session's skill entries list, was not \
                     refused with `{INVALID_PARAMS}`",
                    uri_param(&exchange).unwrap_or_default()
                ),
            );
        } else if let Some(code) =
            code.filter(|code| ![INVALID_PARAMS, INTERNAL_ERROR, METHOD_NOT_FOUND].contains(code))
        {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`{DIRECTORY_READ}` was refused with `{code}`; the codes the extension assigns \
                     are `{INVALID_PARAMS}` (not a directory) and `{INTERNAL_ERROR}`"
                ),
            );
        }
    }
}

/// `SKILL-067`: a server declaring `directoryRead` lists every directory of the
/// skills it serves as files — a skill's root, every directory its manifest
/// puts a file under, and every child a directory result marks as one.
pub(in crate::checks) fn directory_read_every_directory(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let Declaration::Declared(settings) = declaration(context) else {
        return;
    };
    if !directory_read(settings) {
        return;
    }
    let mut directories: BTreeSet<String> = entries::known_directories(context);
    for exchange in context.exchanges_for(DIRECTORY_READ) {
        let Some(uri) = uri_param(&exchange) else {
            continue;
        };
        if directories.contains(uri) {
            sink.examined();
            if exchange.result.is_none() {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "`{DIRECTORY_READ}` of {uri:?}, a directory of a skill the server serves, \
                         was refused"
                    ),
                );
            }
        }
        // A child the result marks as a directory is one too, for later reads.
        let children = exchange
            .result
            .and_then(|result| result.get("resources")?.as_array());
        for child in children.into_iter().flatten() {
            if child.get("mimeType").and_then(Value::as_str) == Some("inode/directory")
                && let Some(child) = child.get("uri").and_then(Value::as_str)
            {
                directories.insert(child.to_owned());
            }
        }
    }
}
