// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Dated MCP protocol revision identifiers.
//!
//! MCP versions its specification with date-shaped identifiers such as `2025-11-25`.
//! [`ProtocolRevision`] models these as an ordered triple rather than a string so that
//! revision ranges ("introduced in", "removed in") compare correctly. The type is an
//! *identifier*, not a calendar date: it validates shape and field ranges, but does not
//! reject impossible calendar dates (such as February 30th), because the authoritative
//! set of revisions is whatever the specification publishes.

use core::fmt;
use core::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The `2025-11-25` protocol revision — current at the time this crate's built-in
/// registry was extracted.
pub const REVISION_2025_11_25: ProtocolRevision = ProtocolRevision {
    year: 2025,
    month: 11,
    day: 25,
};

/// A dated MCP protocol revision identifier (`YYYY-MM-DD`).
///
/// Ordering is chronological, which gives revision-range logic ("does requirement X
/// apply at revision Y?") its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolRevision {
    year: u16,
    month: u8,
    day: u8,
}

impl ProtocolRevision {
    /// Builds a revision from its parts.
    ///
    /// Returns `None` when a field is outside its identifier range (`month` not in
    /// `1..=12`, `day` not in `1..=31`, or `year` zero).
    #[must_use]
    pub const fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if year == 0 || month == 0 || month > 12 || day == 0 || day > 31 {
            return None;
        }
        Some(Self { year, month, day })
    }
}

impl fmt::Display for ProtocolRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Error produced when parsing a [`ProtocolRevision`] from text.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseRevisionError {
    /// The rejected input, for diagnostics.
    pub input: String,
}

impl fmt::Display for ParseRevisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid protocol revision {:?}: expected YYYY-MM-DD with month 01-12 and day 01-31",
            self.input
        )
    }
}

impl core::error::Error for ParseRevisionError {}

impl FromStr for ProtocolRevision {
    type Err = ParseRevisionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseRevisionError {
            input: s.to_owned(),
        };
        let bytes = s.as_bytes();
        // Exactly `YYYY-MM-DD`: 10 ASCII bytes with fixed hyphen positions. Checking
        // shape before parsing keeps inputs like "2025-1-25" or "2025/11/25" out.
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(err());
        }
        let all_digits =
            |range: core::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
        if !all_digits(0..4) || !all_digits(5..7) || !all_digits(8..10) {
            return Err(err());
        }
        let year: u16 = s[0..4].parse().map_err(|_| err())?;
        let month: u8 = s[5..7].parse().map_err(|_| err())?;
        let day: u8 = s[8..10].parse().map_err(|_| err())?;
        Self::new(year, month, day).ok_or_else(err)
    }
}

impl Serialize for ProtocolRevision {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ProtocolRevision {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parses_and_displays_round_trip() {
        let revision: ProtocolRevision = "2025-11-25".parse().unwrap();
        assert_eq!(revision, REVISION_2025_11_25);
        assert_eq!(revision.to_string(), "2025-11-25");
    }

    #[test]
    fn ordering_is_chronological() {
        let older: ProtocolRevision = "2025-03-26".parse().unwrap();
        let newer: ProtocolRevision = "2025-11-25".parse().unwrap();
        let next: ProtocolRevision = "2026-07-28".parse().unwrap();
        assert!(older < newer);
        assert!(newer < next);
    }

    #[test]
    fn rejects_malformed_inputs() {
        for input in [
            "",
            "2025-11",
            "2025-11-25T00",
            "2025/11/25",
            "2025-13-01",
            "2025-00-10",
            "2025-12-00",
            "2025-12-32",
            "0000-01-01",
            "20251-1-25",
            "2025-1-25",
            "2025-11-2x",
        ] {
            assert!(
                input.parse::<ProtocolRevision>().is_err(),
                "should reject {input:?}"
            );
        }
    }

    #[test]
    fn serde_uses_string_form() {
        let json = serde_json::to_string(&REVISION_2025_11_25).unwrap();
        assert_eq!(json, "\"2025-11-25\"");
        let back: ProtocolRevision = serde_json::from_str(&json).unwrap();
        assert_eq!(back, REVISION_2025_11_25);
    }

    proptest! {
        #[test]
        fn display_parse_round_trips(year in 1u16..=9999, month in 1u8..=12, day in 1u8..=31) {
            let revision = ProtocolRevision::new(year, month, day).unwrap();
            let text = revision.to_string();
            let parsed: ProtocolRevision = text.parse().unwrap();
            prop_assert_eq!(parsed, revision);
        }

        #[test]
        fn ordering_matches_tuple_ordering(
            a in (1u16..=9999, 1u8..=12, 1u8..=31),
            b in (1u16..=9999, 1u8..=12, 1u8..=31),
        ) {
            let ra = ProtocolRevision::new(a.0, a.1, a.2).unwrap();
            let rb = ProtocolRevision::new(b.0, b.1, b.2).unwrap();
            prop_assert_eq!(ra.cmp(&rb), a.cmp(&b));
        }
    }

    #[test]
    fn rejects_inputs_failing_exactly_one_shape_clause() {
        // Each input violates a single disjunct of the shape checks, pinning the
        // || operators in from_str against && mutations.
        for input in [
            "2025x11-25", // length ok, first hyphen wrong
            "2025-11x25", // length ok, second hyphen wrong
            "202x-11-25", // year digits wrong only
            "2025-x1-25", // month digits wrong only
            "2025-11-x5", // day digits wrong only
        ] {
            assert!(
                input.parse::<ProtocolRevision>().is_err(),
                "should reject {input:?}"
            );
        }
    }

    #[test]
    fn parse_error_message_names_the_input_and_expectation() {
        let error = "garbage".parse::<ProtocolRevision>().unwrap_err();
        let message = error.to_string();
        assert!(message.contains("garbage"), "{message}");
        assert!(message.contains("YYYY-MM-DD"), "{message}");
    }

    #[test]
    fn rejects_sign_characters_that_integer_parsing_would_accept() {
        // The digit pre-checks are NOT redundant with `.parse::<u16>()`: Rust's
        // integer FromStr accepts a leading `+`, so without them "+025-11-25" would
        // parse as year 25. These inputs pin each digit clause independently —
        // the downstream parse cannot mask the mutation.
        for input in ["+025-11-25", "2025-+1-25", "2025-11-+5"] {
            assert!(
                input.parse::<ProtocolRevision>().is_err(),
                "should reject {input:?}"
            );
        }
    }
}
