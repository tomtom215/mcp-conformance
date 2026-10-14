// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The sidecar beside each recording: `<id>.meta.json`.
//!
//! It holds what the trace cannot: the tags people gave it, where it came
//! from (recorded, imported from another tool — with what that tool did not
//! record — or a redacted copy of another entry), when it was added and
//! recorded, and a cache of its verdict. The cache is keyed by the trace's
//! SHA-256 *and* the tool's version, so editing the trace or upgrading the
//! validator each make it stale, and a stale cache is recomputed, never read.
//!
//! Members this build does not know are kept and written back unchanged, so
//! a newer workbench's sidecar survives an older one opening the library.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The sidecar format's version.
pub(crate) const FORMAT: u32 = 1;

/// One recording's sidecar. A member it lacks takes its value from
/// [`Meta::default`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Meta {
    /// [`FORMAT`].
    pub(crate) format: u32,
    /// Tags, sorted and without duplicates.
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    /// Where the recording came from.
    #[serde(default)]
    pub(crate) source: Source,
    /// When it entered the library (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) added_at: Option<String>,
    /// When it was recorded (RFC 3339), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) recorded_at: Option<String>,
    /// The last verdict computed for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) verdict_cache: Option<Cache>,
    /// Members this build does not know, kept as they were.
    #[serde(flatten)]
    pub(crate) unknown: Map<String, Value>,
}

impl Default for Meta {
    /// An empty sidecar, of this build's [`FORMAT`].
    fn default() -> Self {
        Self {
            format: FORMAT,
            tags: Vec::new(),
            source: Source::default(),
            added_at: None,
            recorded_at: None,
            verdict_cache: None,
            unknown: Map::new(),
        }
    }
}

/// Where a recording came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Source {
    /// `recorded` (a trace written by this toolkit), `imported` (converted
    /// from another tool's format) or `redacted` (a redacted copy of another
    /// entry).
    #[serde(default = "recorded")]
    pub(crate) kind: String,
    /// The format it was imported from (`trace`, `mcpsnoop`, `inspector`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) importer: Option<String>,
    /// The file name it was imported from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) imported_from: Option<String>,
    /// What the source format does not record, so the trace cannot show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) not_recorded: Vec<String>,
    /// The entry this is a redacted copy of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) redacted_from: Option<String>,
    /// What the redaction replaced, one line per rule.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) redactions: Vec<String>,
}

fn recorded() -> String {
    "recorded".to_owned()
}

impl Default for Source {
    fn default() -> Self {
        Self {
            kind: recorded(),
            importer: None,
            imported_from: None,
            not_recorded: Vec::new(),
            redacted_from: None,
            redactions: Vec::new(),
        }
    }
}

/// A verdict, and what it was computed from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cache {
    /// The trace's SHA-256, lowercase hex.
    pub(crate) sha256: String,
    /// The tool version that computed it.
    pub(crate) tool_version: String,
    /// The validator's reports (its JSON report format), one per judged
    /// revision; empty when the trace could not be judged.
    #[serde(default)]
    pub(crate) reports: Vec<Value>,
    /// Why the trace could not be judged, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) unjudged: Option<String>,
}

impl Meta {
    /// A sidecar for a recording of this kind.
    pub(crate) fn new(kind: &str) -> Self {
        Self {
            source: Source {
                kind: kind.to_owned(),
                ..Source::default()
            },
            ..Self::default()
        }
    }

    /// The sidecar as written: pretty JSON, one trailing newline.
    pub(crate) fn to_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Replaces the tags with `tags`, trimmed, without empty ones or
    /// duplicates, sorted.
    pub(crate) fn set_tags<'a>(&mut self, tags: impl IntoIterator<Item = &'a str>) {
        let mut tags: Vec<String> = tags
            .into_iter()
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        tags.sort();
        tags.dedup();
        self.tags = tags;
    }

    /// The cache, when it was computed from this trace by this build.
    pub(crate) fn fresh_cache(&self, sha256: &str) -> Option<&Cache> {
        self.verdict_cache
            .as_ref()
            .filter(|cache| cache.sha256 == sha256 && cache.tool_version == tool_version())
    }
}

/// This build's version: what a cache is keyed by beside the digest.
pub(crate) const fn tool_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_sidecar_round_trips_and_keeps_what_it_does_not_know() {
        let text = r#"{"format":1,"tags":["b","a"],"source":{"kind":"imported","importer":"mcpsnoop"},"future":{"x":1}}"#;
        let meta: Meta = serde_json::from_str(text).unwrap();
        assert_eq!(meta.source.importer.as_deref(), Some("mcpsnoop"));
        assert_eq!(
            meta.unknown.get("future"),
            Some(&serde_json::json!({"x":1}))
        );
        let again: Meta = serde_json::from_str(&meta.to_text()).unwrap();
        assert_eq!(again, meta);
        assert!(meta.to_text().ends_with("}\n"));
    }

    #[test]
    fn an_empty_sidecar_is_a_recording_with_nothing_known() {
        let meta: Meta = serde_json::from_str("{}").unwrap();
        assert_eq!(meta.format, FORMAT);
        assert_eq!(Meta::default().format, FORMAT);
        assert_eq!(meta, Meta::default());
        assert_eq!(meta.source.kind, "recorded");
        assert_eq!(meta.tags.len(), 0);
        assert_eq!(Meta::new("imported").source.kind, "imported");
    }

    #[test]
    fn tags_are_trimmed_deduplicated_and_sorted() {
        let mut meta = Meta::default();
        meta.set_tags([" sdk-go", "", "ci", "sdk-go ", "  "]);
        assert_eq!(meta.tags, ["ci", "sdk-go"]);
    }

    #[test]
    fn a_cache_is_fresh_only_for_the_same_trace_and_build() {
        let mut meta = Meta::default();
        assert!(meta.fresh_cache("abc").is_none());
        meta.verdict_cache = Some(Cache {
            sha256: "abc".into(),
            tool_version: tool_version().into(),
            reports: Vec::new(),
            unjudged: None,
        });
        assert!(meta.fresh_cache("abc").is_some());
        assert!(meta.fresh_cache("abd").is_none());
        if let Some(cache) = &mut meta.verdict_cache {
            cache.tool_version = "0.0.0-other".into();
        }
        assert!(meta.fresh_cache("abc").is_none());
    }

    #[test]
    fn a_cache_from_a_build_that_named_another_version_or_none_is_stale() {
        for other in ["", "xyzzy"] {
            let meta = Meta {
                verdict_cache: Some(Cache {
                    sha256: "abc".into(),
                    tool_version: other.into(),
                    reports: Vec::new(),
                    unjudged: None,
                }),
                ..Meta::default()
            };
            assert!(meta.fresh_cache("abc").is_none(), "{other:?}");
        }
        assert_eq!(tool_version(), env!("CARGO_PKG_VERSION"));
    }
}
