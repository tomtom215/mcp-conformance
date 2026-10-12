// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Whether a parsed JSON message says what its bytes said.
//!
//! A trace records a message as parsed JSON, and the validator reads it back the
//! same way (`serde_json::Value`, without arbitrary precision). Two things that
//! are valid JSON do not survive that: an integer past 64 bits, or a decimal
//! finer than a double, is read as the nearest double (an id of
//! `12345678901234567890123` comes back as `1.2345678901234568e22`, a
//! non-integer the validator then convicts); and of a repeated member name only
//! the last value is kept. Neither is a matter of spelling — `1E2` and `100.0`
//! are the same value, and the trace keeps the value — so only these are found.
//!
//! The scan runs over bytes that already parsed as JSON, so it can take their
//! well-formedness for granted.

use std::collections::HashSet;

/// Whether `bytes` — valid JSON — read back as a `serde_json::Value` with every
/// number exactly the value written and no member name repeated in an object.
pub(super) fn is_faithful(bytes: &[u8]) -> bool {
    // One entry per open container: the member names an object has seen, or
    // `None` for an array.
    let mut open: Vec<Option<HashSet<String>>> = Vec::new();
    let mut expect_name = false;
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'"' => {
                let end = string_end(bytes, at);
                if expect_name && let Some(Some(names)) = open.last_mut() {
                    let Ok(name) = serde_json::from_slice::<String>(&bytes[at..end]) else {
                        return false;
                    };
                    if !names.insert(name) {
                        return false;
                    }
                }
                at = end;
                continue;
            }
            b'-' | b'0'..=b'9' => {
                let end = bytes[at..]
                    .iter()
                    .position(|byte| {
                        !matches!(byte, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                    })
                    .map_or(bytes.len(), |length| at + length);
                if !number_is_exact(&bytes[at..end]) {
                    return false;
                }
                at = end;
                continue;
            }
            b'{' => {
                open.push(Some(HashSet::new()));
                expect_name = true;
            }
            b'[' => open.push(None),
            b'}' | b']' => {
                open.pop();
            }
            b',' => expect_name = matches!(open.last(), Some(Some(_))),
            b':' => expect_name = false,
            _ => {}
        }
        at += 1;
    }
    true
}

/// The index just past the string whose opening quote is at `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut at = start + 1;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// Whether the number `token` is, as parsed, exactly the value it spells.
fn number_is_exact(token: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(token) else {
        return false;
    };
    let Ok(parsed) = text.parse::<serde_json::Number>() else {
        return false;
    };
    match (decimal(text), decimal(&parsed.to_string())) {
        (Some(written), Some(read)) => written == read,
        _ => false,
    }
}

/// A number's value as sign, significant digits and power of ten — equal for
/// two spellings exactly when their values are equal. `None` for an exponent
/// past what an `i64` holds.
fn decimal(text: &str) -> Option<(bool, String, i64)> {
    let (negative, text) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text, "0"), |(mantissa, exponent)| (mantissa, exponent));
    let mut exponent: i64 = exponent.trim_start_matches('+').parse().ok()?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    exponent = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    let significant = digits.trim_end_matches('0');
    if significant.is_empty() {
        // Zero, whatever its sign or spelling.
        return Some((false, String::new(), 0));
    }
    let trailing = i64::try_from(digits.len() - significant.len()).ok()?;
    Some((
        negative,
        significant.to_owned(),
        exponent.checked_add(trailing)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_is_not_a_change_of_value() {
        for faithful in [
            r#"{"id":1,"x":1E2,"y":0.1,"z":-0.0,"w":-0,"v":1.50,"u":-12e-3}"#,
            r#"{"max":18446744073709551615,"min":-9223372036854775808}"#,
            r#"[1,[2,{"a":{"a":1}}],{"a":2},"\"a\""]"#,
            r#"{"a":"1e999 is text, so is {\"a\":1,\"a\":2}","b":{"a":1},"c":[{"a":1},{"a":1}]}"#,
            "0.30000000000000004",
            r#""\\""#,
        ] {
            assert!(is_faithful(faithful.as_bytes()), "{faithful}");
        }
    }

    #[test]
    fn numbers_the_trace_cannot_hold_and_repeated_names_are_found() {
        for altered in [
            r#"{"jsonrpc":"2.0","id":12345678901234567890123,"method":"ping"}"#,
            "18446744073709551616",
            "-9223372036854775809",
            "1.00000000000000000001",
            "1e-400",
            r#"{"id":1,"id":2}"#,
            r#"{"a":1,"a":2}"#,
            r#"[{"b":{"c":1,"c":1}}]"#,
            r#"{"a":[1,2],"b":0,"a":3}"#,
        ] {
            assert!(!is_faithful(altered.as_bytes()), "{altered}");
        }
    }

    #[test]
    fn values_compare_by_value() {
        assert_eq!(decimal("100.0"), decimal("1E2"));
        assert_eq!(decimal("1e+2"), decimal("100"));
        assert_eq!(decimal("-0.0"), decimal("0"));
        assert_eq!(decimal("0.000"), Some((false, String::new(), 0)));
        assert_eq!(decimal("-1.25e1"), Some((true, "125".to_owned(), -1)));
        assert_ne!(decimal("12"), decimal("-12"));
        assert_ne!(decimal("12"), decimal("120"));
        assert_eq!(decimal("1e99999999999999999999"), None);
        assert!(!number_is_exact(b"1e99999999999999999999"));
        assert!(!number_is_exact(b"\xff"));
    }

    #[test]
    fn an_unterminated_string_ends_the_scan() {
        assert_eq!(string_end(b"\"ab\\\"c", 0), 6);
        assert_eq!(string_end(b"\"ab\"c", 0), 4);
    }
}
