// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Trace documents are untrusted input by design; parsing must never panic —
//! and neither may judging what parsed: every parseable input is also run
//! through the full validation engine against the builtin registry, so a
//! panic in any check under hostile payloads is this target's to find.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use mcp_conformance_core::requirement::Registry;
use mcp_trace_validator::engine::validate;
use mcp_trace_validator::reader::{Limits, parse_trace};

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    Registry::builtin_2025_11_25().expect("the embedded registry loads (build-time data)")
});

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = core::str::from_utf8(data)
        && let Ok(events) = parse_trace(text, &Limits::default())
    {
        let _ = validate(&REGISTRY, &events);
    }
});
