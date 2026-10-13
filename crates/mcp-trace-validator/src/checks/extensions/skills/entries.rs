// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a skill entry says about its skill: its URI, its name, and the manifest
//! of its files (`resources`), as `skills/list` and `skills/get` return it.
//!
//! The manifest's digests and sizes are not compared with the bytes a later
//! `resources/read` returns: an entry is a point-in-time snapshot, and the page
//! itself names a skill updated after its entry was fetched as an ordinary
//! cause of a mismatch, so a recording cannot tell a wrong manifest from a stale
//! one. What an entry must say about itself — whatever was read afterwards — is
//! judged here.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The file every skill has at its root.
const SKILL_MD: &str = "/SKILL.md";

/// The scheme the page recommends.
const SKILL_SCHEME: &str = "skill://";

/// Most files one skill should list (the page's Limits table).
const MAX_FILES: usize = 512;

/// Most bytes one skill's files should total: 16 MiB.
const MAX_BYTES: u64 = 16_777_216;

/// Whether `uri` names a `SKILL.md`.
pub(in crate::checks) fn is_skill_md(uri: &str) -> bool {
    uri.ends_with(SKILL_MD) && uri.len() > SKILL_MD.len()
}

/// A skill's root directory: its `SKILL.md` URI without `/SKILL.md`.
pub(in crate::checks) fn root(uri: &str) -> Option<&str> {
    uri.strip_suffix(SKILL_MD).filter(|root| !root.is_empty())
}

/// The final segment of a skill's root — its name, by the path rule.
pub(in crate::checks) fn final_segment(root: &str) -> &str {
    root.rsplit('/').next().unwrap_or(root)
}

/// One skill entry as a result carried it.
#[derive(Debug, Clone, Copy)]
pub(in crate::checks) struct Entry<'a> {
    /// The seq of the result that carried it.
    pub seq: u64,
    /// The entry.
    pub entry: &'a Value,
}

impl<'a> Entry<'a> {
    /// The entry's `uri`.
    pub(in crate::checks) fn uri(&self) -> Option<&'a str> {
        self.entry.get("uri").and_then(Value::as_str)
    }

    /// A string member of the entry's `frontmatter`.
    pub(in crate::checks) fn frontmatter(&self, key: &str) -> Option<&'a str> {
        self.entry.get("frontmatter")?.get(key)?.as_str()
    }

    /// The manifest, when `resources` is an array.
    fn manifest(&self) -> Option<&'a Vec<Value>> {
        self.entry.get("resources").and_then(Value::as_array)
    }

    /// The manifest's digest for `uri`, when it lists one.
    pub(in crate::checks) fn digest_of(&self, uri: &str) -> Option<&'a str> {
        self.manifest()?
            .iter()
            .find(|file| file.get("uri").and_then(Value::as_str) == Some(uri))?
            .get("digest")?
            .as_str()
    }
}

/// Every skill entry the session's `skills/list` and `skills/get` results carried,
/// in result order.
pub(in crate::checks) fn entries<'a>(context: &'a TraceContext<'a>) -> Vec<Entry<'a>> {
    let mut out = Vec::new();
    for exchange in context.exchanges() {
        let Some(result) = exchange.result else {
            continue;
        };
        let seq = exchange.response.seq;
        match exchange.method {
            "skills/list" => {
                let listed = result.get("skills").and_then(Value::as_array);
                out.extend(
                    listed
                        .into_iter()
                        .flatten()
                        .map(|entry| Entry { seq, entry }),
                );
            }
            "skills/get" => {
                if let Some(entry) = result.get("skill") {
                    out.push(Entry { seq, entry });
                }
            }
            _ => {}
        }
    }
    out
}

/// Every URI the session shows to be a file: each entry's `SKILL.md`, each file
/// a manifest lists, and each child a directory result does not mark as a
/// directory.
pub(in crate::checks) fn known_files(context: &TraceContext<'_>) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    for entry in entries(context) {
        files.extend(entry.uri().map(str::to_owned));
        let listed = entry.manifest().into_iter().flatten();
        files.extend(listed.filter_map(|file| file.get("uri")?.as_str().map(str::to_owned)));
    }
    for exchange in context.exchanges_for("resources/directory/read") {
        let children = exchange
            .result
            .and_then(|result| result.get("resources")?.as_array());
        for child in children.into_iter().flatten() {
            if child.get("mimeType").and_then(Value::as_str) != Some("inode/directory")
                && let Some(uri) = child.get("uri").and_then(Value::as_str)
            {
                files.insert(uri.to_owned());
            }
        }
    }
    files
}

/// Every directory of a skill served as files: each root of an entry with a
/// manifest, and every directory between that root and a file it lists.
pub(in crate::checks) fn known_directories(context: &TraceContext<'_>) -> BTreeSet<String> {
    let mut directories = BTreeSet::new();
    for entry in entries(context) {
        let (Some(root), Some(manifest)) = (entry.uri().and_then(root), entry.manifest()) else {
            continue;
        };
        directories.insert(root.to_owned());
        for file in manifest.iter().filter_map(|file| file.get("uri")?.as_str()) {
            let Some(relative) = file
                .strip_prefix(root)
                .and_then(|rest| rest.strip_prefix('/'))
            else {
                continue;
            };
            let mut directory = root.to_owned();
            let mut segments: Vec<&str> = relative.split('/').collect();
            segments.pop(); // the file itself
            for segment in segments {
                directory.push('/');
                directory.push_str(segment);
                directories.insert(directory.clone());
            }
        }
    }
    directories
}

/// `SKILL-007`: an entry's `uri` is its skill's `SKILL.md`, at the skill's root.
pub(in crate::checks) fn entry_uri_is_skill_md(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for entry in entries(context) {
        sink.examined();
        if !entry.uri().is_some_and(is_skill_md) {
            sink.push(
                Some(entry.seq),
                format!(
                    "a skill entry's `uri` is {}, not the URI of a `SKILL.md` at the skill's root",
                    entry
                        .entry
                        .get("uri")
                        .map_or_else(|| "absent".to_owned(), ToString::to_string)
                ),
            );
        }
    }
}

/// `SKILL-011`: skills are served under `skill://`.
pub(in crate::checks) fn skill_scheme(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for entry in entries(context) {
        let Some(uri) = entry.uri() else {
            continue;
        };
        sink.examined();
        if !uri.starts_with(SKILL_SCHEME) {
            sink.push(
                Some(entry.seq),
                format!("the skill {uri:?} is not served under the `skill://` scheme"),
            );
        }
    }
}

/// Entries whose `uri` names a `SKILL.md`, with the skill's root.
fn rooted<'a>(context: &'a TraceContext<'a>) -> impl Iterator<Item = (Entry<'a>, &'a str)> {
    entries(context)
        .into_iter()
        .filter_map(|entry| Some((entry, root(entry.uri()?)?)))
}

/// `SKILL-012`, `SKILL-027`: a skill's path ends in its `name`.
pub(in crate::checks) fn final_segment_is_name(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (entry, root) in rooted(context) {
        let Some(name) = entry.frontmatter("name") else {
            continue;
        };
        sink.examined();
        if final_segment(root) != name {
            sink.push(
                Some(entry.seq),
                format!(
                    "the skill at {root:?} is named {name:?}, but its path ends in {:?}",
                    final_segment(root)
                ),
            );
        }
    }
}

/// `SKILL-013`: a skill's name — its path's final segment — follows the Agent
/// Skills naming rules (agentskills.io/specification, read 2026-10-13): 1 to
/// 64 characters, lowercase letters, digits and hyphens only, no hyphen first
/// or last, no two hyphens in a row.
pub(in crate::checks) fn name_follows_naming_rules(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (entry, root) in rooted(context) {
        let name = final_segment(root);
        sink.examined();
        if let Some(broken) = naming_rule_broken(name) {
            sink.push(
                Some(entry.seq),
                format!("the skill name {name:?} breaks the Agent Skills naming rules: {broken}"),
            );
        }
    }
}

/// The first naming rule `name` breaks, if any.
pub(in crate::checks) fn naming_rule_broken(name: &str) -> Option<&'static str> {
    let length = name.chars().count();
    if !(1..=64).contains(&length) {
        Some("it must be 1 to 64 characters")
    } else if !name.chars().all(|character| {
        character == '-' || (character.is_alphanumeric() && !character.is_uppercase())
    }) {
        Some("only lowercase letters, digits and hyphens are allowed")
    } else if name.starts_with('-') || name.ends_with('-') {
        Some("it must not start or end with a hyphen")
    } else if name.contains("--") {
        Some("it must not contain consecutive hyphens")
    } else {
        None
    }
}

/// RFC 3986 `unreserved` and `sub-delims`, the characters `reg-name` and `pchar`
/// share besides percent-encodings.
fn unreserved_or_sub_delim(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=".contains(&byte)
}

/// Whether `text` is made of `allowed` characters and well-formed `%XX` escapes.
fn rfc3986_text(text: &str, allowed: fn(u8) -> bool) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let escape = bytes.get(index + 1..index + 3);
            if !escape.is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit)) {
                return false;
            }
            index += 3;
        } else if allowed(bytes[index]) {
            index += 1;
        } else {
            return false;
        }
    }
    true
}

/// The `/`-separated segments of a `skill://` root, authority first.
fn skill_segments(root: &str) -> Option<Vec<&str>> {
    Some(root.strip_prefix(SKILL_SCHEME)?.split('/').collect())
}

/// `SKILL-014`: a `skill://` URI's first segment is an RFC 3986 `reg-name`.
pub(in crate::checks) fn authority_reg_name(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (entry, root) in rooted(context) {
        let Some(authority) = skill_segments(root).and_then(|segments| segments.first().copied())
        else {
            continue;
        };
        sink.examined();
        if !rfc3986_text(authority, unreserved_or_sub_delim) {
            sink.push(
                Some(entry.seq),
                format!(
                    "the skill path's first segment {authority:?} is not a valid RFC 3986 `reg-name`"
                ),
            );
        }
    }
}

/// `SKILL-015`: the segments between a `skill://` URI's authority and its name
/// are RFC 3986 path segments.
pub(in crate::checks) fn prefix_segments(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (entry, root) in rooted(context) {
        let Some(segments) = skill_segments(root) else {
            continue;
        };
        let middle = segments.len().saturating_sub(1);
        for segment in segments.iter().take(middle).skip(1) {
            sink.examined();
            if segment.is_empty()
                || !rfc3986_text(segment, |byte| {
                    unreserved_or_sub_delim(byte) || b":@".contains(&byte)
                })
            {
                sink.push(
                    Some(entry.seq),
                    format!("the skill path's prefix segment {segment:?} is not a valid URI path segment"),
                );
            }
        }
    }
}

/// `SKILL-034`: every entry carries `resources`, an array or `"dynamic"`.
pub(in crate::checks) fn entry_resources_form(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for entry in entries(context) {
        sink.examined();
        let resources = entry.entry.get("resources");
        if !resources.is_some_and(|value| value.is_array() || value.as_str() == Some("dynamic")) {
            sink.push(
                Some(entry.seq),
                format!(
                    "the skill entry {:?} carries `resources` {}, neither an array nor \"dynamic\"",
                    entry.uri().unwrap_or_default(),
                    resources.map_or_else(|| "absent".to_owned(), ToString::to_string)
                ),
            );
        }
    }
}

/// `SKILL-035`: a manifest lists `SKILL.md` itself, and each file once.
pub(in crate::checks) fn manifest_lists_skill_md(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for entry in entries(context) {
        let (Some(uri), Some(manifest)) = (entry.uri(), entry.manifest()) else {
            continue;
        };
        sink.examined();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for file in manifest.iter().filter_map(|file| file.get("uri")?.as_str()) {
            *counts.entry(file).or_default() += 1;
        }
        if !counts.contains_key(uri) {
            sink.push(
                Some(entry.seq),
                format!("the manifest of {uri:?} does not list the skill's own `SKILL.md`"),
            );
        }
        for (file, count) in counts.iter().filter(|(_, count)| **count > 1) {
            sink.push(
                Some(entry.seq),
                format!("the manifest of {uri:?} lists {file:?} {count} times, not once"),
            );
        }
    }
}

/// `SKILL-036`: a manifest lists only the skill's `SKILL.md` and files under
/// its directory.
pub(in crate::checks) fn manifest_within_skill(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (entry, root) in rooted(context) {
        let Some(manifest) = entry.manifest() else {
            continue;
        };
        let inside = format!("{root}/");
        for file in manifest.iter().filter_map(|file| file.get("uri")?.as_str()) {
            sink.examined();
            if !file.starts_with(&inside) {
                sink.push(
                    Some(entry.seq),
                    format!("the manifest of the skill at {root:?} lists {file:?}, outside its directory"),
                );
            }
        }
    }
}

/// `SKILL-037`: each manifest entry carries `size`, a byte count.
pub(in crate::checks) fn manifest_entry_size(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for entry in entries(context) {
        for file in entry.manifest().into_iter().flatten() {
            sink.examined();
            if !file.get("size").is_some_and(Value::is_u64) {
                sink.push(
                    Some(entry.seq),
                    format!(
                        "the manifest entry for {:?} carries no `size` in bytes",
                        file.get("uri").and_then(Value::as_str).unwrap_or_default()
                    ),
                );
            }
        }
    }
}

/// `SKILL-042`: a served skill stays within 512 files and 16 MiB.
pub(in crate::checks) fn within_limits(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for entry in entries(context) {
        let Some(manifest) = entry.manifest() else {
            continue;
        };
        sink.examined();
        let bytes: u64 = manifest
            .iter()
            .filter_map(|file| file.get("size")?.as_u64())
            .fold(0, u64::saturating_add);
        if manifest.len() > MAX_FILES || bytes > MAX_BYTES {
            sink.push(
                Some(entry.seq),
                format!(
                    "the skill {:?} lists {} files totalling {bytes} bytes, beyond the limits of \
                     {MAX_FILES} files and {MAX_BYTES} bytes",
                    entry.uri().unwrap_or_default(),
                    manifest.len()
                ),
            );
        }
    }
}

/// `SKILL-029`: within one listing — a `skills/list` without a cursor and the
/// pages that follow it — no two skills share a name.
pub(in crate::checks) fn listing_names_unique(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut pages: Vec<_> = context.exchanges_for("skills/list").collect();
    pages.sort_by_key(|exchange| exchange.request.seq);
    let mut listings: Vec<Vec<(u64, &str, &str)>> = Vec::new();
    for page in pages {
        let continues = page
            .params
            .and_then(|params| params.get("cursor"))
            .is_some();
        if !continues || listings.is_empty() {
            listings.push(Vec::new());
        }
        let seq = page.response.seq;
        let listed = page
            .result
            .and_then(|result| result.get("skills")?.as_array());
        let named = listed.into_iter().flatten().filter_map(|entry| {
            let entry = Entry { seq, entry };
            Some((seq, entry.frontmatter("name")?, entry.uri()?))
        });
        if let Some(listing) = listings.last_mut() {
            listing.extend(named);
        }
    }
    for listing in listings.iter().filter(|listing| listing.len() > 1) {
        let mut first: BTreeMap<&str, &str> = BTreeMap::new();
        for (seq, name, uri) in listing {
            sink.examined();
            match first.get(name) {
                Some(other) if other != uri => sink.push(
                    Some(*seq),
                    format!(
                        "two skills in one listing share the name {name:?}: {other:?} and {uri:?}"
                    ),
                ),
                Some(_) => {}
                None => {
                    first.insert(name, uri);
                }
            }
        }
    }
}
