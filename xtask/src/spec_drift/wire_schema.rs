// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The vendored protocol schemas against `main` (ADR-0020).
//!
//! Each `spec-schema/<revision>.schema.json` must equal the specification
//! repository's `schema/<revision>/schema.json` on `main` byte for byte — the
//! errata are applied when the validator loads a schema, never to the file —
//! and the sentence that makes the schema normative must still be on the page
//! `WireSchema::section` names.

use mcp_conformance_core::wire_schema::{AUTHORITY_QUOTE, WIRE_SCHEMAS};

use super::{fetch, load_sources, normalize, quote_present, raw_base};

/// Where `main` publishes a revision's schema.
fn upstream_url(path: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/{path}"
    )
}

/// How many vendored schemas (and their authority quotes) have drifted, after
/// saying so; `Err(())` when one could not be fetched.
pub(super) fn verify() -> Result<u32, ()> {
    let mut drifted = 0u32;
    for schema in WIRE_SCHEMAS {
        let url = upstream_url(schema.upstream_path);
        let upstream = fetch(&url).map_err(|message| {
            eprintln!("xtask: spec-drift — cannot fetch {url}: {message}");
        })?;
        if upstream == schema.json {
            eprintln!(
                "xtask: spec-drift — {}: the vendored schema matches main",
                schema.revision
            );
        } else {
            eprintln!(
                "xtask: spec-drift — {}: the vendored schema differs from {url}. Copy the new \
                 file into crates/mcp-conformance-core/spec-schema/, update the commit there \
                 and in wire_schema.rs, and re-check every erratum against schema.ts",
                schema.revision
            );
            drifted += 1;
        }
        let revision = schema.revision.to_string();
        let page = schema.section.split('#').next().unwrap_or_default();
        let sources = load_sources(&revision).map_err(|message| {
            eprintln!("xtask: spec-drift — {revision}: {message}");
        })?;
        let Some(file) = sources.in_scope.get(page) else {
            eprintln!(
                "xtask: spec-drift — {revision}: the schema's authority section {} names a \
                 page sources.json does not list in scope",
                schema.section
            );
            return Err(());
        };
        let page_url = format!("{}/{file}", raw_base(&revision));
        let text = fetch(&page_url).map_err(|message| {
            eprintln!("xtask: spec-drift — cannot fetch {page_url}: {message}");
        })?;
        if !quote_present(&normalize(&text), AUTHORITY_QUOTE) {
            eprintln!(
                "xtask: spec-drift — {}: the schema's authority sentence is no longer on \
                 {page_url}:\n  {AUTHORITY_QUOTE:?}",
                schema.revision
            );
            drifted += 1;
        }
    }
    eprintln!(
        "xtask: spec-drift — {} vendored protocol schema(s) compared with main, {drifted} drifted",
        WIRE_SCHEMAS.len()
    );
    Ok(drifted)
}
