// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask fuzz-targets` — every fuzz target source is declared as a
//! `[[bin]]`, so `cargo fuzz` can see it.
//!
//! A target nothing runs is not a weaker guarantee than the standard promises;
//! it is no guarantee at all, delivered with the paperwork of one. That is not
//! hypothetical here: `registry_set_multi` was written on 2026-07-27 to cover
//! "the only engine path whose *shape* is attacker-influenced", complete with a
//! seed corpus — and the weekly job's hardcoded target list was never extended,
//! so it sat unrun. Nothing failed, because nothing was watching the list.
//!
//! Two changes close that, and this is the offline half. The workflow now takes
//! its list from `cargo fuzz list` instead of writing target names out, so what
//! is declared is what runs; this gate checks the other end of that chain — that
//! what *exists* is what is declared — because a source file `fuzz/Cargo.toml`
//! never learned about is invisible to `cargo fuzz list` and would sit unrun in
//! exactly the same way.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::BTreeSet;
use std::fs;

/// Verifies the correspondence; `true` when every source is declared.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    let sources = match sources(&root) {
        Ok(sources) => sources,
        Err(error) => {
            eprintln!("xtask: fuzz-targets — {error}");
            return false;
        }
    };
    let manifest = root.join("fuzz/Cargo.toml");
    let declared = match fs::read_to_string(&manifest) {
        Ok(text) => declared(&text),
        Err(error) => {
            eprintln!(
                "xtask: fuzz-targets — cannot read {}: {error}",
                manifest.display()
            );
            return false;
        }
    };
    let undeclared: Vec<&String> = sources.difference(&declared).collect();
    if !undeclared.is_empty() {
        eprintln!(
            "xtask: fuzz-targets — these targets exist but fuzz/Cargo.toml declares no \
             `[[bin]]` for them, so `cargo fuzz` cannot run them: {undeclared:?}"
        );
        return false;
    }
    let missing: Vec<&String> = declared.difference(&sources).collect();
    if !missing.is_empty() {
        eprintln!(
            "xtask: fuzz-targets — fuzz/Cargo.toml declares targets with no source under \
             fuzz/fuzz_targets: {missing:?}"
        );
        return false;
    }
    eprintln!(
        "xtask: fuzz-targets — all {} targets are declared and runnable",
        sources.len()
    );
    true
}

/// The stem of every `.rs` file under `fuzz/fuzz_targets`.
fn sources(root: &std::path::Path) -> Result<BTreeSet<String>, String> {
    let directory = root.join("fuzz/fuzz_targets");
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    let mut found = BTreeSet::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
            .path();
        if path.extension().is_some_and(|extension| extension == "rs")
            && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
        {
            found.insert(stem.to_owned());
        }
    }
    if found.is_empty() {
        return Err(format!("no targets under {}", directory.display()));
    }
    Ok(found)
}

/// Every `name` declared under a `[[bin]]` table in `manifest`.
///
/// Read line-wise rather than with a TOML parser: the manifest is this
/// repository's own, its shape is fixed, and the question — which names a
/// `[[bin]]` introduces — is answerable without one.
fn declared(manifest: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut in_bin = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_bin = line == "[[bin]]";
            continue;
        }
        if in_bin
            && let Some(value) = line.strip_prefix("name")
            && let Some(name) = value.trim_start().strip_prefix('=')
        {
            found.insert(name.trim().trim_matches('"').to_owned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_tree_agrees() {
        assert!(run(), "the committed fuzz targets and manifest disagree");
    }

    #[test]
    fn names_are_read_from_bin_tables_only() {
        let manifest = "\
[package]
name = \"mcp-conformance-fuzz\"

[[bin]]
name = \"trace_parse\"
path = \"fuzz_targets/trace_parse.rs\"

[dependencies]
name = \"not-a-target\"

[[bin]]
name = \"canonical_json\"
";
        assert_eq!(
            declared(manifest),
            BTreeSet::from(["trace_parse".to_owned(), "canonical_json".to_owned()]),
            "the package name and a dependency key must not be read as targets"
        );
    }
}
