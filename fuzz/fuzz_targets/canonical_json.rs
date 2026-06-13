// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Canonicalization is a **stable normal form**: for every parseable JSON
//! document, `canonical(parse(canonical(v))) == canonical(v)`. This is the
//! idempotence invariant — the same one the `canonical_form_is_a_parse_fixpoint`
//! property test asserts — and it is the *correct* universal property for a
//! canonicalizer that deliberately folds representations (JCS maps `-0.0`,
//! `0`, and `2.0` to `0`, `0`, `2`). The earlier invariant here —
//! `parse(canonical(v)) == v` over `serde_json::Value` — was false by design
//! for any such value: it survived only because the never-CI-run fuzz job had
//! not yet generated a `-0.0` (third audit, 2026-06-13; the corpus seed
//! `seed-negative-zero-fold` pins the exact input). The canonicalizer
//! was always correct; the harness had contradicted its own unit test.
//!
//! Two properties are checked on every input: the canonical form is valid
//! JSON (it parses), and re-canonicalizing it is a no-op (a fixed point at
//! the string level).

#![no_main]

use libfuzzer_sys::fuzz_target;
use mcp_conformance_core::canonical::to_canonical_string;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = serde_json::from_slice::<Value>(data) {
        let canonical = to_canonical_string(&value);
        match serde_json::from_str::<Value>(&canonical) {
            Ok(reparsed) => assert_eq!(
                to_canonical_string(&reparsed),
                canonical,
                "canonicalization is not idempotent: re-canonicalizing the canonical \
                 form changed it"
            ),
            Err(error) => panic!("canonical form is not valid JSON: {error}\n{canonical}"),
        }
    }
});
