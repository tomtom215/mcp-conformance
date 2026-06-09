// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Canonicalization must be total and round-trip exact: for every parseable JSON
//! document, parse(canonical(v)) == v. This asserts the invariant, not mere survival.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mcp_conformance_core::canonical::to_canonical_string;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = serde_json::from_slice::<Value>(data) {
        let canonical = to_canonical_string(&value);
        match serde_json::from_str::<Value>(&canonical) {
            Ok(reparsed) => assert_eq!(reparsed, value, "canonical form changed the value"),
            Err(error) => panic!("canonical form is not valid JSON: {error}\n{canonical}"),
        }
    }
});
