// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The protocol's own JSON Schema for each revision this crate describes.
//!
//! The specification calls its TypeScript schema "the source of truth for all
//! protocol messages and structures", and publishes a JSON Schema generated
//! from it "for use with various automated tooling". Each [`WireSchema`] is that
//! JSON Schema, vendored byte for byte (`spec-schema/` in this crate, under the
//! specification's licence), with the commit it was taken from and the
//! [`Erratum`] list of places where the generated file says less than
//! `schema.ts` allows. This module is data only: validating a message against
//! the schema needs a JSON Schema engine, which `mcp-trace-validator` carries
//! (ADR-0020), not this crate.
//!
//! ```
//! use mcp_conformance_core::revision::REVISION_2026_07_28;
//! use mcp_conformance_core::wire_schema;
//!
//! let schema = wire_schema::for_revision(REVISION_2026_07_28).expect("vendored");
//! let parsed: serde_json::Value = serde_json::from_str(schema.json)?;
//! assert!(parsed["$defs"]["CallToolResult"].is_object());
//! assert!(schema.url().ends_with("/schema/2026-07-28/schema.json"));
//! # Ok::<(), serde_json::Error>(())
//! ```

use crate::requirement::SourceRef;
use crate::revision::{ProtocolRevision, REVISION_2025_11_25, REVISION_2026_07_28};

/// The specification-repository commit every vendored schema was copied from.
pub const UPSTREAM_COMMIT: &str = "3778656b2e0ba6725035e816fe3085374cbaa736";

/// One revision's vendored `schema.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WireSchema {
    /// The revision the schema describes.
    pub revision: ProtocolRevision,
    /// The file's text, exactly as published upstream.
    pub json: &'static str,
    /// Its path in the specification repository.
    pub upstream_path: &'static str,
    /// Where the generated file is stricter than `schema.ts`, and the correction.
    pub errata: &'static [Erratum],
    /// The page and anchor of the specification's statement that the schema is
    /// normative (`basic#schema`), in registry `source.section` form.
    pub section: &'static str,
}

/// The specification's statement of the schema's authority.
///
/// Verbatim at every revision with a vendored schema (`cargo xtask spec-drift`
/// re-verifies it). It carries no RFC 2119 keyword, which is why schema validity is reported
/// beside the requirement rows rather than as one of them (ADR-0020).
pub const AUTHORITY_QUOTE: &str =
    "This is the source of truth for all protocol messages and structures.";

impl WireSchema {
    /// The vendored file's permanent address: the upstream path at
    /// [`UPSTREAM_COMMIT`].
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "https://github.com/modelcontextprotocol/modelcontextprotocol/blob/{UPSTREAM_COMMIT}/{}",
            self.upstream_path
        )
    }

    /// The specification's statement that this schema is normative, as a
    /// registry-shaped source: [`Self::section`] and [`AUTHORITY_QUOTE`].
    #[must_use]
    pub fn source(&self) -> SourceRef {
        SourceRef {
            section: self.section.to_owned(),
            quote: AUTHORITY_QUOTE.to_owned(),
        }
    }
}

/// A place where the generated JSON Schema contradicts `schema.ts`.
///
/// The generator writes a TypeScript `number` as JSON Schema `integer` unless
/// the field is annotated otherwise, and drops `null` from one union. Where
/// `schema.ts` — the stated source of truth — admits a value the JSON Schema
/// refuses, judging by the JSON Schema would fail a conforming message, so the
/// value at [`pointer`](Self::pointer) is replaced before validation. Each
/// erratum names the upstream value it replaces, and a test fails when the
/// vendored file no longer holds it, so a refreshed schema cannot be silently
/// "corrected" in a place upstream has already changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Erratum {
    /// RFC 6901 JSON Pointer into the schema document.
    pub pointer: &'static str,
    /// The value upstream publishes there, as JSON.
    pub upstream: &'static str,
    /// The value `schema.ts` implies, as JSON.
    pub corrected: &'static str,
    /// The `schema.ts` declaration that decides it.
    pub reason: &'static str,
}

/// `ElicitResult.content`'s values: `schema.ts` declares
/// `{ [key: string]: string | number | boolean | string[] }`, so a form answer
/// to a `NumberSchema` field may be `95.5`; the generated schema says `integer`.
const ELICIT_CONTENT_NUMBER: Erratum = Erratum {
    pointer: "/$defs/ElicitResult/properties/content/additionalProperties/anyOf/1/type",
    upstream: r#"["string","integer","boolean"]"#,
    corrected: r#"["string","number","boolean"]"#,
    reason: "schema.ts: ElicitResult.content is { [key: string]: string | number | boolean | string[] }",
};

const ERRATA_2025_11_25: &[Erratum] = &[ELICIT_CONTENT_NUMBER];

const ERRATA_2026_07_28: &[Erratum] = &[
    ELICIT_CONTENT_NUMBER,
    Erratum {
        pointer: "/$defs/JSONValue/anyOf/2/type",
        upstream: r#"["string","integer","boolean"]"#,
        corrected: r#"["string","number","boolean","null"]"#,
        reason: "schema.ts: JSONValue is string | number | boolean | null | JSONObject | JSONArray",
    },
];

/// Every vendored schema, oldest revision first.
pub const WIRE_SCHEMAS: &[WireSchema] = &[
    WireSchema {
        revision: REVISION_2025_11_25,
        json: include_str!("../spec-schema/2025-11-25.schema.json"),
        upstream_path: "schema/2025-11-25/schema.json",
        errata: ERRATA_2025_11_25,
        section: "basic#schema",
    },
    WireSchema {
        revision: REVISION_2026_07_28,
        json: include_str!("../spec-schema/2026-07-28.schema.json"),
        upstream_path: "schema/2026-07-28/schema.json",
        errata: ERRATA_2026_07_28,
        section: "basic/index#schema",
    },
];

/// The vendored schema for `revision`, if this build has one.
#[must_use]
pub fn for_revision(revision: ProtocolRevision) -> Option<&'static WireSchema> {
    WIRE_SCHEMAS
        .iter()
        .find(|schema| schema.revision == revision)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::requirement::BUILTIN_REVISIONS;

    #[test]
    fn every_builtin_revision_has_a_schema_and_no_other_does() {
        let schemas: Vec<_> = WIRE_SCHEMAS.iter().map(|schema| schema.revision).collect();
        assert_eq!(schemas, BUILTIN_REVISIONS);
    }

    #[test]
    fn each_schema_is_draft_2020_12_with_definitions() {
        for schema in WIRE_SCHEMAS {
            let parsed: Value = serde_json::from_str(schema.json).unwrap();
            assert_eq!(
                parsed["$schema"], "https://json-schema.org/draft/2020-12/schema",
                "{}",
                schema.revision
            );
            assert!(
                parsed["$defs"]["JSONRPCRequest"].is_object(),
                "{}",
                schema.revision
            );
            assert!(
                schema.upstream_path.contains(&schema.revision.to_string()),
                "{}",
                schema.upstream_path
            );
        }
    }

    /// The guard the [`Erratum`] doc promises: each correction replaces exactly
    /// the value upstream publishes, and the correction parses.
    #[test]
    fn each_erratum_names_the_value_upstream_publishes() {
        for schema in WIRE_SCHEMAS {
            let parsed: Value = serde_json::from_str(schema.json).unwrap();
            for erratum in schema.errata {
                let found = parsed.pointer(erratum.pointer).unwrap_or_else(|| {
                    panic!(
                        "{}: {} resolves to nothing",
                        schema.revision, erratum.pointer
                    )
                });
                let upstream: Value = serde_json::from_str(erratum.upstream).unwrap();
                assert_eq!(found, &upstream, "{}: {}", schema.revision, erratum.pointer);
                let corrected: Value = serde_json::from_str(erratum.corrected).unwrap();
                assert_ne!(corrected, upstream);
            }
        }
    }

    #[test]
    fn the_url_pins_the_commit() {
        let schema = for_revision(REVISION_2025_11_25).unwrap();
        assert_eq!(
            schema.url(),
            format!(
                "https://github.com/modelcontextprotocol/modelcontextprotocol/blob/{UPSTREAM_COMMIT}/schema/2025-11-25/schema.json"
            )
        );
        assert_eq!(
            schema.source().url(REVISION_2025_11_25),
            "https://modelcontextprotocol.io/specification/2025-11-25/basic#schema"
        );
        assert!(for_revision(ProtocolRevision::new(2025, 6, 18).unwrap()).is_none());
    }
}
