// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Trace documents are untrusted input by design; parsing must never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mcp_trace_validator::reader::{Limits, parse_trace};

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = core::str::from_utf8(data) {
        let _ = parse_trace(text, &Limits::default());
    }
});
