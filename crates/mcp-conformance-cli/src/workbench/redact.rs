// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A redacted copy of a recording.
//!
//! Recordings carry what was sent: an imported one can hold `Authorization`
//! and cookies, and any recording holds payloads in full. A redaction is a
//! set of [`Rules`]; applying them yields a new trace, which the library adds
//! as a new entry beside the original — **never in place** — with a sidecar
//! saying what was replaced and from which entry. Every replaced value
//! becomes the string [`REDACTED`], so the copy is still a trace the
//! validator reads.
//!
//! The rules work on each line as JSON, so a member this build does not know
//! is carried over (and its strings are still scrubbed of literal secrets).
//! A line that is not JSON cannot be redacted with confidence, so the whole
//! redaction is refused rather than carrying it over.

use serde_json::{Map, Value};

/// What a replaced value becomes.
pub(crate) const REDACTED: &str = "[redacted]";

/// The headers redacted unless the request names others: the ones that
/// carry credentials.
pub(crate) const DEFAULT_HEADERS: [&str; 4] = [
    "authorization",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

/// What to replace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Rules {
    /// Header names, compared without regard to ASCII case.
    pub(crate) headers: Vec<String>,
    /// JSON pointers (RFC 6901) into each message payload — of a batch, into
    /// each element; a `*` segment stands for every member or element.
    pub(crate) pointers: Vec<String>,
    /// Replace the value of every member of every `_meta` object.
    pub(crate) meta: bool,
    /// Literal strings replaced wherever they occur: any string value,
    /// header value or member name.
    pub(crate) strings: Vec<String>,
}

/// A redacted trace, and what was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Redacted {
    pub(crate) text: String,
    /// One line per rule that replaced something: the rule and how often.
    /// Never the replaced values themselves.
    pub(crate) summary: Vec<String>,
}

#[derive(Debug, Default)]
struct Counts {
    headers: usize,
    pointers: Vec<usize>,
    meta: usize,
    strings: Vec<usize>,
}

/// Why rules cannot be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactError(pub(crate) String);

impl Rules {
    /// Checks the rules before anything is applied.
    fn check(&self) -> Result<(), RedactError> {
        if let Some(pointer) = self
            .pointers
            .iter()
            .find(|pointer| !pointer.starts_with('/'))
        {
            return Err(RedactError(format!(
                "{pointer:?} is not a JSON pointer: one starts with '/', e.g. /params/arguments/token"
            )));
        }
        if self.strings.iter().any(String::is_empty) {
            return Err(RedactError("an empty string cannot be redacted".to_owned()));
        }
        if self.headers.is_empty()
            && self.pointers.is_empty()
            && !self.meta
            && self.strings.is_empty()
        {
            return Err(RedactError(
                "no rule: name a header, a pointer, _meta or a string".to_owned(),
            ));
        }
        Ok(())
    }
}

/// One pointer segment, unescaped (`~1` is `/`, `~0` is `~`).
fn segments(pointer: &str) -> Vec<String> {
    pointer
        .split('/')
        .skip(1)
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .collect()
}

/// Replaces what `path` names under `value`; returns how many values.
fn at_pointer(value: &mut Value, path: &[String]) -> usize {
    let Some((first, rest)) = path.split_first() else {
        *value = Value::String(REDACTED.to_owned());
        return 1;
    };
    match value {
        Value::Object(members) if first == "*" => members
            .values_mut()
            .map(|child| at_pointer(child, rest))
            .sum(),
        Value::Array(items) if first == "*" => {
            items.iter_mut().map(|child| at_pointer(child, rest)).sum()
        }
        Value::Object(members) => members
            .get_mut(first.as_str())
            .map_or(0, |child| at_pointer(child, rest)),
        Value::Array(items) => first
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get_mut(index))
            .map_or(0, |child| at_pointer(child, rest)),
        _ => 0,
    }
}

/// Replaces every `_meta` member's value under `value`.
fn meta_values(value: &mut Value) -> usize {
    match value {
        Value::Object(members) => {
            let mut count = 0;
            for (name, child) in members.iter_mut() {
                if name == "_meta"
                    && let Value::Object(meta) = child
                {
                    for inner in meta.values_mut() {
                        *inner = Value::String(REDACTED.to_owned());
                        count += 1;
                    }
                } else {
                    count += meta_values(child);
                }
            }
            count
        }
        Value::Array(items) => items.iter_mut().map(meta_values).sum(),
        _ => 0,
    }
}

/// Replaces `needle` in every string and member name under `value`.
fn scrub(value: &mut Value, needle: &str) -> usize {
    match value {
        Value::String(text) => {
            let count = text.matches(needle).count();
            *text = text.replace(needle, REDACTED);
            count
        }
        Value::Array(items) => items.iter_mut().map(|item| scrub(item, needle)).sum(),
        Value::Object(members) => {
            let mut count = 0;
            let renamed: Map<String, Value> = core::mem::take(members)
                .into_iter()
                .map(|(name, mut child)| {
                    count += scrub(&mut child, needle);
                    if name.contains(needle) {
                        count += name.matches(needle).count();
                        (name.replace(needle, REDACTED), child)
                    } else {
                        (name, child)
                    }
                })
                .collect();
            *members = renamed;
            count
        }
        _ => 0,
    }
}

fn redact_line(line: &mut Value, rules: &Rules, pointers: &[Vec<String>], counts: &mut Counts) {
    let kind = line
        .get("kind")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    if kind.as_deref() == Some("http")
        && let Some(Value::Object(headers)) = line.get_mut("headers")
    {
        for (name, value) in headers.iter_mut() {
            if rules
                .headers
                .iter()
                .any(|header| header.eq_ignore_ascii_case(name))
            {
                *value = Value::String(REDACTED.to_owned());
                counts.headers += 1;
            }
        }
    }
    if kind.as_deref() == Some("message")
        && let Some(payload) = line.get_mut("payload")
    {
        for (index, path) in pointers.iter().enumerate() {
            counts.pointers[index] += match &mut *payload {
                Value::Array(parts) => parts.iter_mut().map(|part| at_pointer(part, path)).sum(),
                part => at_pointer(part, path),
            };
        }
        if rules.meta {
            counts.meta += meta_values(payload);
        }
    }
    for (index, needle) in rules.strings.iter().enumerate() {
        counts.strings[index] += scrub(line, needle);
    }
}

/// Applies `rules` to the trace `text`.
pub(crate) fn redact(text: &str, rules: &Rules) -> Result<Redacted, RedactError> {
    rules.check()?;
    let pointers: Vec<Vec<String>> = rules.pointers.iter().map(|p| segments(p)).collect();
    let mut counts = Counts {
        pointers: vec![0; pointers.len()],
        strings: vec![0; rules.strings.len()],
        ..Counts::default()
    };
    let mut out = String::with_capacity(text.len());
    for (number, line) in text.lines().enumerate() {
        let mut value: Value = serde_json::from_str(line).map_err(|error| {
            RedactError(format!(
                "line {} is not JSON ({error}); a line that cannot be read cannot be redacted, \
                 so nothing was copied",
                number + 1
            ))
        })?;
        redact_line(&mut value, rules, &pointers, &mut counts);
        out.push_str(&serde_json::to_string(&value).unwrap_or_default());
        out.push('\n');
    }
    let mut summary = Vec::new();
    if !rules.headers.is_empty() {
        summary.push(format!(
            "headers {}: {} values",
            rules.headers.join(", "),
            counts.headers
        ));
    }
    for (pointer, count) in rules.pointers.iter().zip(&counts.pointers) {
        summary.push(format!("pointer {pointer}: {count} values"));
    }
    if rules.meta {
        summary.push(format!("_meta members: {} values", counts.meta));
    }
    for (index, count) in counts.strings.iter().enumerate() {
        // The string itself is the secret: the summary names it by position.
        summary.push(format!("literal string {}: {count} occurrences", index + 1));
    }
    Ok(Redacted { text: out, summary })
}

#[cfg(test)]
mod tests;
