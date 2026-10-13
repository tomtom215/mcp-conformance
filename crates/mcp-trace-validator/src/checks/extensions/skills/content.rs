// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a skill's `SKILL.md` says and how it is described as a resource: its
//! YAML frontmatter, read back from `resources/read`, against the entry that
//! describes it; and the metadata `resources/list` gives a `skill://` `SKILL.md`.
//!
//! **Frontmatter is read, not parsed.** The validator carries no YAML parser,
//! and the clauses need only the top level: which keys the frontmatter has, and
//! the values of those written as plain scalars on their key's line. Anything
//! nested, multi-line or otherwise beyond that is compared by key alone, so a
//! difference inside it goes unjudged — a missed finding, never a false one.

use core::fmt::Write as _;

use serde_json::Value;

use super::super::super::FindingSink;
use super::super::super::support::decode_base64;
use super::entries::{self, Entry, final_segment, is_skill_md, root};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// One top-level frontmatter key, with its value when it is a plain scalar.
type Field = (String, Option<String>);

/// The top-level keys of the YAML frontmatter `text` begins with, or `None`
/// when it does not begin with frontmatter: a `---` line, then lines up to the
/// next `---` line.
pub(in crate::checks) fn frontmatter(text: &str) -> Option<Vec<Field>> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut fields = Vec::new();
    for line in lines {
        if line.trim_end() == "---" {
            return Some(fields);
        }
        let top_level = line.chars().next().is_some_and(|first| {
            !first.is_whitespace() && !matches!(first, '#' | '-' | '?' | '[' | '{')
        });
        if !top_level {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = unquote(key.trim()).to_owned();
        fields.push((key, scalar(value.trim())));
    }
    None
}

/// A plain or quoted scalar written on the key's line; `None` for anything else
/// (an empty value introducing a block, a block scalar, a flow collection, an
/// anchor, alias or tag).
fn scalar(value: &str) -> Option<String> {
    if value.is_empty() || value.starts_with(['|', '>', '[', '{', '&', '*', '!']) {
        return None;
    }
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        // An escape inside a quoted scalar would need YAML's own rules to read.
        let inner = &value[1..value.len() - 1];
        return (!inner.contains('\\') && !inner.contains("''")).then(|| inner.to_owned());
    }
    // A ` #` starts a comment after a plain scalar.
    Some(
        value
            .split(" #")
            .next()
            .unwrap_or(value)
            .trim_end()
            .to_owned(),
    )
}

fn unquote(key: &str) -> &str {
    key.trim_matches(['"', '\''])
}

/// Every `SKILL.md` a `resources/read` returned, as `(seq, uri, bytes)`: text
/// content as written, blob content decoded. A `SKILL.md` is any content item
/// whose URI names one under `skill://`, or whose URI a skill entry gave.
fn skill_md_reads(context: &TraceContext<'_>) -> Vec<(u64, String, String)> {
    let known: Vec<String> = entries::entries(context)
        .iter()
        .filter_map(|entry| entry.uri().map(str::to_owned))
        .collect();
    let mut reads = Vec::new();
    for exchange in context.exchanges_for("resources/read") {
        let contents = exchange
            .result
            .and_then(|result| result.get("contents")?.as_array());
        for content in contents.into_iter().flatten() {
            let Some(uri) = content.get("uri").and_then(Value::as_str) else {
                continue;
            };
            let skill = is_skill_md(uri)
                && (uri.starts_with("skill://") || known.iter().any(|known| known == uri));
            if !skill {
                continue;
            }
            let bytes = content
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| content.get("blob")?.as_str().and_then(decode_base64));
            if let Some(bytes) = bytes {
                reads.push((exchange.response.seq, uri.to_owned(), bytes));
            }
        }
    }
    reads
}

/// `SKILL-008`: a `SKILL.md` begins with YAML frontmatter that has `name` and
/// `description`.
pub(in crate::checks) fn skill_md_frontmatter(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, uri, text) in skill_md_reads(context) {
        sink.examined();
        let Some(fields) = frontmatter(&text) else {
            sink.push(
                Some(seq),
                format!("{uri:?} does not begin with YAML frontmatter (a `---` line, then fields, then `---`)"),
            );
            continue;
        };
        let missing: Vec<&str> = ["name", "description"]
            .into_iter()
            .filter(|key| !fields.iter().any(|(field, _)| field == key))
            .collect();
        if !missing.is_empty() {
            sink.push(
                Some(seq),
                format!("the frontmatter of {uri:?} lacks {}", missing.join(" and ")),
            );
        }
    }
}

/// `SKILL-026`: an entry's `frontmatter` is its `SKILL.md`'s frontmatter.
///
/// Judged only where the bytes read are the bytes the entry describes — their
/// SHA-256 is the digest its manifest gives for the `SKILL.md` — since a skill
/// may change between its listing and a read, which the page allows.
pub(in crate::checks) fn frontmatter_identical(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let described = entries::entries(context);
    for (seq, uri, text) in skill_md_reads(context) {
        let digest = format!(
            "sha256:{}",
            hex(&crate::html::digest::sha256(text.as_bytes()))
        );
        let Some(entry) = described.iter().rev().find(|entry| {
            entry.uri() == Some(uri.as_str()) && entry.digest_of(&uri) == Some(digest.as_str())
        }) else {
            continue;
        };
        let (Some(fields), Some(declared)) = (
            frontmatter(&text),
            entry.entry.get("frontmatter").and_then(Value::as_object),
        ) else {
            continue;
        };
        sink.examined();
        let mut differences = Vec::new();
        for (key, value) in &fields {
            match (declared.get(key), value) {
                (None, _) => differences.push(format!("`{key}` is missing from the entry")),
                (Some(Value::String(entry_value)), Some(file_value))
                    if entry_value != file_value =>
                {
                    differences.push(format!(
                        "`{key}` is {entry_value:?} in the entry but {file_value:?} in the file"
                    ));
                }
                _ => {}
            }
        }
        for key in declared.keys() {
            if !fields.iter().any(|(field, _)| field == key) {
                differences.push(format!("`{key}` is in the entry but not the file"));
            }
        }
        if !differences.is_empty() {
            sink.push(
                Some(seq),
                format!(
                    "the entry's `frontmatter` for {uri:?} (from seq {}) is not the file's: {}",
                    entry.seq,
                    differences.join("; ")
                ),
            );
        }
    }
}

/// Lower-case hex of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(2 * bytes.len()), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Every `skill://` `SKILL.md` described as a resource: `resources/list`
/// entries, and `resources/read` contents, as `(seq, resource, listed)`.
fn skill_md_resources<'a>(context: &'a TraceContext<'a>) -> Vec<(u64, &'a Value, bool)> {
    let mut out = Vec::new();
    for exchange in context.exchanges() {
        let (member, listed) = match exchange.method {
            "resources/list" => ("resources", true),
            "resources/read" => ("contents", false),
            _ => continue,
        };
        let items = exchange
            .result
            .and_then(|result| result.get(member)?.as_array());
        for item in items.into_iter().flatten() {
            let uri = item.get("uri").and_then(Value::as_str).unwrap_or_default();
            if uri.starts_with("skill://") && is_skill_md(uri) {
                out.push((exchange.response.seq, item, listed));
            }
        }
    }
    out
}

/// `SKILL-018`: a `skill://` `SKILL.md` resource is `text/markdown`.
pub(in crate::checks) fn skill_md_mime_type(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, resource, _) in skill_md_resources(context) {
        sink.examined();
        let mime = resource.get("mimeType").and_then(Value::as_str);
        if mime != Some("text/markdown") {
            sink.push(
                Some(seq),
                format!(
                    "{:?} is described with `mimeType` {}, not \"text/markdown\"",
                    resource
                        .get("uri")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    mime.map_or_else(|| "absent".to_owned(), |mime| format!("{mime:?}"))
                ),
            );
        }
    }
}

/// The entry describing `uri`, the latest the session saw.
fn entry_for<'a>(described: &'a [Entry<'a>], uri: &str) -> Option<&'a Entry<'a>> {
    described
        .iter()
        .rev()
        .find(|entry| entry.uri() == Some(uri))
}

/// `SKILL-019`: a listed `skill://` `SKILL.md` resource is named from its
/// frontmatter's `name` — the entry's, where the session saw one, else the path's
/// final segment, which the path rule makes equal.
pub(in crate::checks) fn skill_md_resource_name(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let described = entries::entries(context);
    for (seq, resource, listed) in skill_md_resources(context) {
        let Some(uri) = resource
            .get("uri")
            .and_then(Value::as_str)
            .filter(|_| listed)
        else {
            continue;
        };
        let expected = entry_for(&described, uri)
            .and_then(|entry| entry.frontmatter("name"))
            .or_else(|| root(uri).map(final_segment));
        let Some(expected) = expected else {
            continue;
        };
        sink.examined();
        let name = resource.get("name").and_then(Value::as_str);
        if name != Some(expected) {
            sink.push(
                Some(seq),
                format!(
                    "the resource {uri:?} is named {}, not {expected:?} from its frontmatter",
                    name.map_or_else(|| "nothing".to_owned(), |name| format!("{name:?}"))
                ),
            );
        }
    }
}

/// `SKILL-020`: a listed `skill://` `SKILL.md` resource's `description` is its
/// frontmatter's, judged where the session also carried the skill's entry.
pub(in crate::checks) fn skill_md_resource_description(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let described = entries::entries(context);
    for (seq, resource, listed) in skill_md_resources(context) {
        let Some(uri) = resource
            .get("uri")
            .and_then(Value::as_str)
            .filter(|_| listed)
        else {
            continue;
        };
        let Some(expected) =
            entry_for(&described, uri).and_then(|entry| entry.frontmatter("description"))
        else {
            continue;
        };
        sink.examined();
        let description = resource.get("description").and_then(Value::as_str);
        if description != Some(expected) {
            sink.push(
                Some(seq),
                format!(
                    "the resource {uri:?} is described as {}, not as its frontmatter's {expected:?}",
                    description.map_or_else(|| "nothing".to_owned(), |text| format!("{text:?}"))
                ),
            );
        }
    }
}
