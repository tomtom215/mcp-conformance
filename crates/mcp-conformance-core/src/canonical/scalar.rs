// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Scalar serialization for canonical JSON: the ECMAScript number form RFC
//! 8785 §3.2.2.3 requires, plus delegation to `serde_json` for the rest.

use serde_json::Value;

pub(super) fn write_scalar(out: &mut String, value: &Value) {
    // Float-armed numbers take the ECMAScript path RFC 8785 §3.2.2.3 requires;
    // integer-armed numbers, strings, booleans, and null delegate to serde_json's
    // compact serializer, which cannot fail for these variants — the defensive
    // fallback keeps the function total without a reachable panic.
    if let Value::Number(number) = value
        && let Some(float) = number
            .as_f64()
            .filter(|_| !number.is_i64() && !number.is_u64())
    {
        out.push_str(&es6_number(float));
        return;
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
