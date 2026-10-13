// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Where a SARIF result points: the trace, as a URI reference, at the line
//! holding the event a finding names.

use core::fmt::Write as _;

use serde_json::{Value, json};

use super::Artifact;

/// The result's location: the trace, at the line holding the event `seq` names.
/// The reader accepts only one event per line with no blank lines, so the event
/// at index `i` is on line `i + 1`.
pub(super) fn location(artifact: Artifact<'_>, seq: Option<u64>) -> Value {
    let Some(uri) = artifact.uri else {
        return json!([]);
    };
    let mut physical = json!({ "artifactLocation": { "uri": uri_reference(uri) } });
    let line = seq.and_then(|seq| {
        artifact
            .events
            .binary_search_by_key(&seq, |event| event.seq)
            .ok()
    });
    if let Some(index) = line {
        physical["region"] = json!({ "startLine": index + 1 });
    }
    json!([{ "physicalLocation": physical }])
}

/// `path` as a URI reference, `/`-separated with every byte outside RFC 3986's
/// unreserved set and `/` percent-encoded.
///
/// A relative path stays relative — the form code scanning resolves against the
/// repository root — and its colons stay encoded, so none can be read as a
/// scheme. An absolute path becomes a `file:` URI (RFC 8089): `/tmp/t.jsonl` is
/// `file:///tmp/t.jsonl`, and Windows' `D:\t.jsonl` is `file:///D:/t.jsonl`, its
/// drive letter intact rather than the relative `D%3A/t.jsonl` it would
/// otherwise read as.
pub(super) fn uri_reference(path: &str) -> String {
    let path = path.replace('\\', "/");
    let bytes = path.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    let (mut out, rest) = if drive {
        (format!("file:///{}:", char::from(bytes[0])), &path[2..])
    } else if path.starts_with('/') {
        ("file://".to_owned(), path.as_str())
    } else {
        (String::with_capacity(path.len()), path.as_str())
    };
    for byte in rest.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}
