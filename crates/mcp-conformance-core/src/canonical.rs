// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Deterministic canonical JSON serialization.
//!
//! Wherever the toolkit compares, hashes, or golden-tests JSON payloads it first
//! canonicalizes them so that semantically equal documents are byte-equal. The
//! canonical form is:
//!
//! - **Object member ordering** per RFC 8785 (JSON Canonicalization Scheme): keys sorted
//!   by their UTF-16 code unit sequences. This differs from naive Rust `str` ordering
//!   (UTF-8 / code-point order) for keys containing supplementary-plane characters —
//!   e.g. U+10000 sorts *before* U+E000 in UTF-16 order, and after it in code-point
//!   order — so the comparison is implemented explicitly.
//! - **No insignificant whitespace.**
//! - **Scalar serialization** (numbers, string escaping) delegated to `serde_json`,
//!   which is deterministic. Full RFC 8785 *scalar* conformance (ECMAScript number
//!   formatting test vectors) is tracked as M1 hardening work in the project plan; key
//!   ordering — the property correctness depends on today — is fully implemented here.
//!
//! The output is stable across platforms and releases for any given `serde_json` major
//! version, which is what golden tests and trace diffing require.

use core::cmp::Ordering;

use serde_json::Value;

/// Serializes a JSON value to its canonical string form.
///
/// Canonicalization never fails: every `serde_json::Value` is representable.
#[must_use]
pub fn to_canonical_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

/// Compares two strings by their UTF-16 code unit sequences, the ordering RFC 8785
/// prescribes for object member names.
#[must_use]
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            write_scalar(out, value);
        }
        Value::Array(items) => {
            out.push('[');
            let mut first = true;
            for item in items {
                if !first {
                    out.push(',');
                }
                first = false;
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut keys: Vec<&String> = members.keys().collect();
            keys.sort_by(|a, b| cmp_utf16(a, b));
            out.push('{');
            let mut first = true;
            for key in keys {
                if !first {
                    out.push(',');
                }
                first = false;
                write_scalar(out, &Value::String(key.clone()));
                out.push(':');
                if let Some(member) = members.get(key) {
                    write_value(out, member);
                }
            }
            out.push('}');
        }
    }
}

fn write_scalar(out: &mut String, value: &Value) {
    // Scalars (and the strings used for keys) delegate to serde_json's compact
    // serializer, which cannot fail for these variants; defensive fallback keeps the
    // function total without a reachable panic.
    match serde_json::to_string(value) {
        Ok(text) => out.push_str(&text),
        Err(_) => out.push_str("null"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    #[test]
    fn sorts_object_keys() {
        let value = json!({"b": 1, "a": {"d": 4, "c": 3}, "0": []});
        assert_eq!(
            to_canonical_string(&value),
            r#"{"0":[],"a":{"c":3,"d":4},"b":1}"#
        );
    }

    #[test]
    fn key_order_is_utf16_not_code_point() {
        // U+10000 (LINEAR B SYLLABLE B008 A) is encoded in UTF-16 as the surrogate pair
        // D800 DC00; U+E000 is a single unit E000. In UTF-16 order D800 < E000, so the
        // supplementary character sorts FIRST — the opposite of code-point order.
        let supplementary = "\u{10000}";
        let private_use = "\u{E000}";
        assert_eq!(cmp_utf16(supplementary, private_use), Ordering::Less);
        assert_eq!(
            private_use.cmp(supplementary),
            Ordering::Less,
            "code-point order disagrees, which is the whole point of this test"
        );

        let value = json!({ private_use: 1, supplementary: 2 });
        let canonical = to_canonical_string(&value);
        let supplementary_at = canonical.find(supplementary).unwrap();
        let private_use_at = canonical.find(private_use).unwrap();
        assert!(supplementary_at < private_use_at, "canonical: {canonical}");
    }

    #[test]
    fn compact_and_stable_for_scalars() {
        for (value, expected) in [
            (json!(null), "null"),
            (json!(true), "true"),
            (json!(-0.0), "-0.0"),
            (json!(12345), "12345"),
            (json!("a\"b\\c\nd"), r#""a\"b\\c\nd""#),
            (json!([]), "[]"),
            (json!({}), "{}"),
        ] {
            assert_eq!(to_canonical_string(&value), expected);
        }
    }

    fn arbitrary_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::from),
            any::<i64>().prop_map(Value::from),
            // Finite floats only: JSON cannot represent NaN/infinity.
            (-1.0e9f64..1.0e9).prop_map(Value::from),
            ".*".prop_map(Value::from),
        ];
        leaf.prop_recursive(4, 32, 8, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
                proptest::collection::btree_map(".*", inner, 0..6)
                    .prop_map(|members| { Value::Object(members.into_iter().collect()) }),
            ]
        })
    }

    proptest! {
        #[test]
        fn canonical_form_round_trips(value in arbitrary_json()) {
            let canonical = to_canonical_string(&value);
            let reparsed: Value = serde_json::from_str(&canonical).unwrap();
            prop_assert_eq!(&reparsed, &value);
        }

        #[test]
        fn canonicalization_is_idempotent(value in arbitrary_json()) {
            let once = to_canonical_string(&value);
            let reparsed: Value = serde_json::from_str(&once).unwrap();
            let twice = to_canonical_string(&reparsed);
            prop_assert_eq!(once, twice);
        }

        #[test]
        fn key_order_is_total_and_consistent(a in ".*", b in ".*") {
            prop_assert_eq!(cmp_utf16(&a, &b), cmp_utf16(&b, &a).reverse());
            if cmp_utf16(&a, &b) == Ordering::Equal {
                prop_assert_eq!(&a, &b);
            }
        }
    }
}
