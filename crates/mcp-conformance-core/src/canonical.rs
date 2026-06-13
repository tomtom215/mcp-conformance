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
/// Nesting is walked with an explicit heap work-stack rather than recursion,
/// so an arbitrarily deep value (e.g. a hostile trace's hundred-thousand-deep
/// array) is bounded by available memory and can never overflow the call
/// stack — a property the determinism foundation must hold on its own, not
/// lean on a caller's parse-depth limit.
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

/// One unit of canonicalization work, processed LIFO. Composites push their
/// pieces in reverse so they emit left to right; the leaf arms write directly.
enum Step<'a> {
    /// Emit a value (a leaf writes itself; a composite expands into more steps).
    Value(&'a Value),
    /// Emit a fixed delimiter (`[`, `]`, `{`, `}`, `,`).
    Delim(&'static str),
    /// Emit an already-rendered `"key":` (escaped member name plus colon).
    Key(String),
}

fn write_value(out: &mut String, root: &Value) {
    let mut stack = vec![Step::Value(root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Delim(text) => out.push_str(text),
            Step::Key(rendered) => out.push_str(&rendered),
            Step::Value(Value::Array(items)) => {
                // Push in reverse so items emit in order: `]`, then each item
                // (comma-prefixed except the first), then `[` on top.
                stack.push(Step::Delim("]"));
                for (index, item) in items.iter().enumerate().rev() {
                    stack.push(Step::Value(item));
                    if index != 0 {
                        stack.push(Step::Delim(","));
                    }
                }
                stack.push(Step::Delim("["));
            }
            Step::Value(Value::Object(members)) => {
                let mut keys: Vec<&String> = members.keys().collect();
                keys.sort_by(|a, b| cmp_utf16(a, b));
                stack.push(Step::Delim("}"));
                for (index, key) in keys.iter().enumerate().rev() {
                    // `members.get` is infallible here — every key came from
                    // `members.keys()` and the map is not mutated — but staying
                    // on the `Option` path keeps the function panic-free by
                    // construction rather than by argument.
                    if let Some(member) = members.get(key.as_str()) {
                        stack.push(Step::Value(member));
                        stack.push(Step::Key(render_key(key)));
                        if index != 0 {
                            stack.push(Step::Delim(","));
                        }
                    }
                }
                stack.push(Step::Delim("{"));
            }
            Step::Value(scalar) => write_scalar(out, scalar),
        }
    }
}

/// Renders an object member name as its canonical `"key":` prefix — the same
/// string escaping the scalar path uses, so keys and string values escape
/// identically.
fn render_key(key: &str) -> String {
    let mut rendered = String::new();
    write_scalar(&mut rendered, &Value::String(key.to_owned()));
    rendered.push(':');
    rendered
}

mod scalar;

use scalar::write_scalar;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    #[test]
    fn deeply_nested_value_canonicalizes_on_a_small_stack() {
        // The determinism foundation must not abort the process on a hostile
        // trace. A recursive walker overflows the call stack on a deep value
        // (an uncatchable SIGABRT, not a panic); the iterative walker bounds
        // depth by heap. Proof: canonicalize a 50k-deep array on a 256 KiB
        // stack — a recursive walker overflows that ~25x over, the iterative
        // one cannot. The deep value is built and dropped on a large-stack
        // thread (serde_json::Value's *drop* is itself recursive), and moved
        // back out of the small-stack closure so it never drops there.
        // Under miri the depth shrinks: the interpreter would take hours at
        // 50k, and the native stack-overflow counterfactual does not
        // translate to miri's stack anyway — there the run checks the
        // walker for UB, not for frame budget.
        const DEPTH: usize = if cfg!(miri) { 500 } else { 50_000 };
        let outcome = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let mut value = Value::Array(vec![]);
                for _ in 0..DEPTH {
                    value = Value::Array(vec![value]);
                }
                let (summary, value) = std::thread::Builder::new()
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        let canonical = to_canonical_string(&value);
                        let summary = (
                            canonical.matches('[').count(),
                            canonical.matches(']').count(),
                            canonical.starts_with("[[") && canonical.ends_with("]]"),
                        );
                        // Move the deep value back out so it is dropped on the
                        // big-stack parent, never on this 256 KiB stack.
                        (summary, value)
                    })
                    .expect("spawn small-stack canonicalize thread")
                    .join()
                    .expect("small-stack thread did not abort");
                drop(value);
                summary
            })
            .expect("spawn big-stack thread")
            .join()
            .expect("big-stack thread");
        // DEPTH wraps plus the innermost empty array's own brackets.
        assert_eq!(outcome, (DEPTH + 1, DEPTH + 1, true));
    }

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
    fn object_key_order_straddles_the_bmp_boundary_in_utf16() {
        // Four keys whose leading UTF-16 units interleave across U+FFFF: two
        // supplementary (lead surrogates D800, DBFF) and two BMP private-use
        // (E000, F8FF). UTF-16 order — D800 < DBFF < E000 < F8FF — puts BOTH
        // supplementary keys first; code-point order would put the BMP keys
        // first. The emitted order must be the UTF-16 one, and this is what a
        // mutation of the sort back to `str::cmp` would break.
        let keys = ["\u{10000}", "\u{10FFFF}", "\u{E000}", "\u{F8FF}"];
        let value = json!({
            keys[2]: 1, keys[0]: 2, keys[3]: 3, keys[1]: 4,
        });
        let canonical = to_canonical_string(&value);
        let positions: Vec<usize> = keys.iter().map(|k| canonical.find(k).unwrap()).collect();
        assert!(
            positions[0] < positions[1]
                && positions[1] < positions[2]
                && positions[2] < positions[3],
            "emitted key order is not UTF-16: {canonical}"
        );
    }

    #[test]
    fn string_escaping_follows_rfc8785_section_3_2_2_2() {
        // String escaping is delegated to serde_json; pin the RFC 8785 rules so
        // a future serde_json change (or a hand-rolled escaper) cannot silently
        // diverge. Two-char escapes where defined, \uXXXX for other C0 control
        // characters, and — the easily-missed rules — DEL (U+007F) and the
        // solidus (/) emitted LITERALLY, supplementary characters as raw UTF-8.
        for (value, expected) in [
            (
                json!("\u{0008}\u{0009}\u{000A}\u{000C}\u{000D}"),
                r#""\b\t\n\f\r""#,
            ),
            // Other C0 controls take lowercase \u00XX (no short escape defined).
            (json!("\u{0000}\u{0001}\u{001F}"), r#""\u0000\u0001\u001f""#),
            (json!("a/b"), r#""a/b""#),            // solidus NOT escaped
            (json!("\u{007F}"), "\"\u{007F}\""),   // DEL emitted literally
            (json!("\u{1F600}"), "\"\u{1F600}\""), // emoji as raw 4-byte UTF-8
        ] {
            let canonical = to_canonical_string(&value);
            assert_eq!(canonical, expected, "input {value}");
        }
        // DEL really is byte 0x7F in the output, not an escape sequence.
        let del = to_canonical_string(&json!("\u{007F}"));
        assert_eq!(del.as_bytes(), [0x22, 0x7F, 0x22]);
    }

    /// The exact input the weekly fuzz job surfaced on its first real CI run
    /// (third audit, 2026-06-13): a nested document carrying `-0.0`. The
    /// canonicalizer folds it to `0` by design (RFC 8785), so the
    /// *representational* round-trip `parse(canonical(v)) == v` does NOT hold
    /// — `Float(-0.0)` becomes integer `0`. The property that does hold, and
    /// the one that matters, is idempotence: canonicalizing the canonical
    /// form is a no-op. Pinned here at `cargo test` speed because the fuzz
    /// job runs only weekly; the corpus seed `seed-negative-zero-fold`
    /// pins it for the fuzzer.
    #[test]
    fn negative_zero_fold_is_idempotent_not_representation_preserving() {
        let value = json!({"b": 1, "a": {"\u{10000}": 2, "\u{e000}": [1.5, -0.0, true, null]}});
        let canonical = to_canonical_string(&value);
        let reparsed: Value = serde_json::from_str(&canonical).expect("canonical is valid JSON");
        // The representational round trip is deliberately lost: -0.0 → 0.
        assert_ne!(
            reparsed, value,
            "JCS folds -0.0 to 0, so Value identity must not hold"
        );
        // The real invariant — a stable normal form — holds.
        assert_eq!(
            to_canonical_string(&reparsed),
            canonical,
            "canonicalization must be idempotent"
        );
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

        /// Numeric value survives canonicalization exactly over EVERY finite
        /// float — normals, subnormals, and the ECMAScript notation boundaries
        /// — not just the normal range. Negative zero is the one exclusion: JCS
        /// folds it to `0` by design (covered by the example tests), so it has
        /// no bit-exact round trip.
        #[test]
        fn canonical_numbers_preserve_value(
            float in any::<f64>()
                .prop_filter("finite, excluding negative zero", |f| {
                    f.is_finite() && f.to_bits() != 0x8000_0000_0000_0000
                })
        ) {
            let canonical = to_canonical_string(&Value::from(float));
            let reparsed: f64 = canonical.parse().unwrap();
            // Bit-exact: every non-negative-zero finite float has one
            // representation, so this is the strictest value preservation (and
            // sidesteps float_cmp). Subnormals and boundaries are now in scope.
            prop_assert_eq!(reparsed.to_bits(), float.to_bits(), "canonical: {}", canonical);
        }

        /// Subnormals specifically: the smallest representable magnitudes, the
        /// values an es6-notation edit is most likely to mishandle.
        #[test]
        fn canonical_preserves_subnormal_floats(mantissa in 1u64..=0x000F_FFFF_FFFF_FFFF) {
            let subnormal = f64::from_bits(mantissa);
            prop_assert!(subnormal.is_finite() && subnormal != 0.0 && !subnormal.is_normal());
            let canonical = to_canonical_string(&Value::from(subnormal));
            let reparsed: f64 = canonical.parse().unwrap();
            prop_assert_eq!(reparsed.to_bits(), subnormal.to_bits(), "canonical: {}", canonical);
        }

        #[test]
        fn key_order_is_total_and_consistent(a in ".*", b in ".*") {
            prop_assert_eq!(cmp_utf16(&a, &b), cmp_utf16(&b, &a).reverse());
            if cmp_utf16(&a, &b) == Ordering::Equal {
                prop_assert_eq!(&a, &b);
            }
        }

        /// `cmp_utf16` really sorts by UTF-16 code units, checked against an
        /// independent reference. Antisymmetry alone (above) holds for
        /// code-point order too; this is what catches a regression of the sort
        /// to `str::cmp`, the classic JCS bug.
        #[test]
        fn cmp_utf16_matches_a_utf16_code_unit_reference(a in ".*", b in ".*") {
            let reference = a
                .encode_utf16()
                .collect::<Vec<u16>>()
                .cmp(&b.encode_utf16().collect::<Vec<u16>>());
            prop_assert_eq!(cmp_utf16(&a, &b), reference);
        }
    }
}
