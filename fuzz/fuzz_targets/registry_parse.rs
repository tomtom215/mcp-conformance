// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! User-supplied registries (`--registry`) are untrusted input; parsing and
//! validation must never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mcp_conformance_core::requirement::Registry;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = core::str::from_utf8(data) {
        let _ = Registry::from_json(text);
    }
});
