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
//! - **Numbers** in the ECMAScript `Number::toString` form RFC 8785 §3.2.2.3 requires,
//!   validated against the RFC's own Appendix B sample table: shortest round-trip
//!   digits (`serde_json`/Ryu provide these), decimal notation only for decimal
//!   exponents in the ES6 window, `0` for negative zero, and explicitly signed
//!   exponents (`1e+21`, not `1e21`). Integer-armed `serde_json` numbers (`i64`/`u64`)
//!   serialize with their exact digits — beyond-double integers are outside JCS's
//!   number model (RFC 8785 Appendix D) and exactness is the safer property for
//!   conformance evidence.
//! - **String escaping** delegated to `serde_json`, which implements the RFC 8785
//!   §3.2.2.2 rules (two-character escapes where defined, `\uXXXX` for other control
//!   characters).
//!
//! Canonically-equal-but-differently-typed numbers fold together here (`-0.0` → `0`,
//! `2.0` → `2`), so canonicalization is a projection onto JCS number semantics, not a
//! bijection on `serde_json::Value` — reparsing canonical output can change a float
//! arm to an integer arm while preserving numeric value. The fixpoint property
//! (canonicalize ∘ parse ∘ canonicalize = canonicalize) is what the property tests
//! pin.

use core::cmp::Ordering;

use serde_json::Value;

/// Serializes a JSON value to its canonical string form.
///
/// Canonicalization never fails: every `serde_json::Value` is representable.
///
/// ```
/// use mcp_conformance_core::canonical::to_canonical_string;
/// use serde_json::json;
///
/// let value = json!({"b": 2.0, "a": [true, -0.0]});
/// assert_eq!(to_canonical_string(&value), r#"{"a":[true,0],"b":2}"#);
/// ```
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
    // Float-armed numbers take the ECMAScript path RFC 8785 §3.2.2.3 requires;
    // integer-armed numbers, strings, booleans, and null delegate to serde_json's
    // compact serializer, which cannot fail for these variants — the defensive
    // fallback keeps the function total without a reachable panic.
    if let Value::Number(number) = value {
        if let Some(float) = number
            .as_f64()
            .filter(|_| !number.is_i64() && !number.is_u64())
        {
            out.push_str(&es6_number(float));
            return;
        }
    }
    match serde_json::to_string(value) {
        Ok(text) => out.push_str(&text),
        Err(_) => out.push_str("null"),
    }
}

/// Serializes a finite `f64` exactly as ECMAScript `Number::toString` does — the RFC
/// 8785 §3.2.2.3 requirement, validated against the RFC's Appendix B sample table.
///
/// `serde_json` (via Ryu) already produces the shortest round-trip digit sequence, the
/// hard part both algorithms share; this function re-notates those digits per the
/// ECMAScript rules: plain decimal only when the decimal-point position `n` satisfies
/// `-6 < n ≤ 21`, exponential with an explicit sign otherwise, and `0` for both zeros.
fn es6_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned(); // Covers -0.0: JCS serializes both zeros as `0`.
    }
    let shortest = serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned());
    let (sign, magnitude) = shortest
        .strip_prefix('-')
        .map_or(("", shortest.as_str()), |rest| ("-", rest));

    // Split mantissa and decimal exponent, then reduce to (digits, n) where the value
    // is 0.<digits> × 10^n with no leading or trailing zero digits.
    let (mantissa, exponent) = magnitude
        .split_once(['e', 'E'])
        .map_or((magnitude, 0_i32), |(mantissa, exponent)| {
            (mantissa, exponent.parse().unwrap_or(0))
        });
    let (int_part, frac_part) = mantissa
        .split_once('.')
        .map_or((mantissa, ""), |(int_part, frac_part)| {
            (int_part, frac_part)
        });
    let digits: String = int_part.chars().chain(frac_part.chars()).collect();
    let leading_zeros = digits.len() - digits.trim_start_matches('0').len();
    // Digit counts of a shortest f64 representation are tiny; the fallbacks are
    // unreachable and exist only to keep the casts total.
    let mut n = i32::try_from(int_part.len()).unwrap_or(0) + exponent;
    n -= i32::try_from(leading_zeros).unwrap_or(0);
    let digits = digits.trim_matches('0');

    let k = i32::try_from(digits.len()).unwrap_or(0);
    let rendered = if k <= n && n <= 21 {
        // All digits before the decimal point, zero-padded: 999999999999999900000.
        let zeros = usize::try_from(n - k).unwrap_or(0);
        format!("{digits}{}", "0".repeat(zeros))
    } else if 0 < n && n <= 21 {
        // Decimal point inside the digits: 333333333.3333333.
        let split = usize::try_from(n).unwrap_or(0);
        format!("{}.{}", &digits[..split], &digits[split..])
    } else if -6 < n && n <= 0 {
        // Leading zeros after the point: 0.000001.
        let zeros = usize::try_from(-n).unwrap_or(0);
        format!("0.{}{digits}", "0".repeat(zeros))
    } else {
        // Exponential with a mandatory sign: 1e+21, 9.999999999999997e-7.
        let exponent = n - 1;
        let mantissa = if digits.len() == 1 {
            digits.to_owned()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        let exponent_sign = if exponent.is_negative() { "-" } else { "+" };
        format!("{mantissa}e{exponent_sign}{}", exponent.abs())
    };
    format!("{sign}{rendered}")
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
            (json!(-0.0), "0"),
            (json!(2.0), "2"),
            (json!(12345), "12345"),
            (json!(-7), "-7"),
            (json!(u64::MAX), "18446744073709551615"),
            (json!("a\"b\\c\nd"), r#""a\"b\\c\nd""#),
            (json!([]), "[]"),
            (json!({}), "{}"),
        ] {
            assert_eq!(to_canonical_string(&value), expected);
        }
    }

    /// RFC 8785 Appendix B, Table 1: "ECMAScript-Compatible JSON Number Serialization
    /// Samples" — every representable row, keyed by IEEE 754 bit pattern exactly as
    /// the RFC prints them (NaN and Infinity are excluded as unrepresentable in
    /// JSON). Fetched from rfc-editor.org/rfc/rfc8785.txt, 2026-06-09.
    #[test]
    fn rfc8785_appendix_b_number_vectors() {
        let vectors: [(u64, &str); 22] = [
            (0x0000_0000_0000_0000, "0"),
            (0x8000_0000_0000_0000, "0"),
            (0x0000_0000_0000_0001, "5e-324"),
            (0x8000_0000_0000_0001, "-5e-324"),
            (0x7fef_ffff_ffff_ffff, "1.7976931348623157e+308"),
            (0xffef_ffff_ffff_ffff, "-1.7976931348623157e+308"),
            (0x4340_0000_0000_0000, "9007199254740992"),
            (0xc340_0000_0000_0000, "-9007199254740992"),
            (0x4430_0000_0000_0000, "295147905179352830000"),
            (0x44b5_2d02_c7e1_4af5, "9.999999999999997e+22"),
            (0x44b5_2d02_c7e1_4af6, "1e+23"),
            (0x44b5_2d02_c7e1_4af7, "1.0000000000000001e+23"),
            (0x444b_1ae4_d6e2_ef4e, "999999999999999700000"),
            (0x444b_1ae4_d6e2_ef4f, "999999999999999900000"),
            (0x444b_1ae4_d6e2_ef50, "1e+21"),
            (0x3eb0_c6f7_a0b5_ed8c, "9.999999999999997e-7"),
            (0x3eb0_c6f7_a0b5_ed8d, "0.000001"),
            (0x41b3_de43_5555_5553, "333333333.3333332"),
            (0x41b3_de43_5555_5554, "333333333.33333325"),
            (0x41b3_de43_5555_5555, "333333333.3333333"),
            (0x41b3_de43_5555_5556, "333333333.3333334"),
            (0x41b3_de43_5555_5557, "333333333.33333343"),
        ];
        for (bits, expected) in vectors {
            let float = f64::from_bits(bits);
            let value = serde_json::Value::from(float);
            assert_eq!(
                to_canonical_string(&value),
                expected,
                "bits {bits:#018x} (value {float:e})"
            );
        }
        // The remaining table row, -0.0000033333333333333333 (0xbecb_f647_612f_3696),
        // exercises the negative decimal-fraction path.
        let value = serde_json::Value::from(f64::from_bits(0xbecb_f647_612f_3696));
        assert_eq!(to_canonical_string(&value), "-0.0000033333333333333333");
    }

    #[test]
    fn es6_notation_boundaries_are_exact() {
        // The ES6 window: plain decimal for -6 < n <= 21, exponential outside.
        for (value, expected) in [
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.5, "1.5"),
            (0.5, "0.5"),
            (-0.5, "-0.5"),
            (-2.5e-8, "-2.5e-8"),
        ] {
            let value = serde_json::Value::from(value);
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
        /// Canonical output is a fixpoint: it reparses, and canonicalizing the
        /// reparse is byte-identical. (Value-level equality is deliberately NOT
        /// asserted: JCS folds `-0.0` to `0` and `2.0` to `2`, which changes the
        /// serde_json number *arm* while preserving the numeric value — and this
        /// property still catches 1-ULP float parse drift, which is why the
        /// workspace pins serde_json's `float_roundtrip` feature.)
        #[test]
        fn canonical_form_is_a_parse_fixpoint(value in arbitrary_json()) {
            let canonical = to_canonical_string(&value);
            let reparsed: Value = serde_json::from_str(&canonical).unwrap();
            prop_assert_eq!(to_canonical_string(&reparsed), canonical);
        }

        /// Numeric value survives canonicalization exactly (compared as f64,
        /// the JCS number model).
        #[test]
        fn canonical_numbers_preserve_value(float in proptest::num::f64::NORMAL) {
            let canonical = to_canonical_string(&Value::from(float));
            let reparsed: f64 = canonical.parse().unwrap();
            // Bit-exact: NORMAL floats have one representation, so this is the
            // strictest form of value preservation (and sidesteps float_cmp).
            prop_assert_eq!(reparsed.to_bits(), float.to_bits(), "canonical: {}", canonical);
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
