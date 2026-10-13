// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Trace documents are untrusted input by design; parsing must never panic —
//! and neither may judging what parsed: every parseable input is also run
//! through the full validation engine against every revision the build ships
//! (each revision's checks and its wire schema), so a panic in any of them
//! under hostile payloads is this target's to find.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use mcp_conformance_core::requirement::{Registry, RegistrySet};
use mcp_trace_validator::engine::validate;
use mcp_trace_validator::reader::{Limits, parse_trace};

static REGISTRIES: LazyLock<Vec<Registry>> = LazyLock::new(|| {
    let set = RegistrySet::builtin().expect("the embedded registries load (build-time data)");
    set.revisions()
        .iter()
        .map(|revision| set.registry(*revision).expect("a described revision projects"))
        .collect()
});

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = core::str::from_utf8(data)
        && let Ok(events) = parse_trace(text, &Limits::default())
    {
        for registry in REGISTRIES.iter() {
            let _ = validate(registry, &events);
        }
    }
});
