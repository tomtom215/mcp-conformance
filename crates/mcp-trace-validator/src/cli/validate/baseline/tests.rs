// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The one place the validator reads the clock: which UTC day a reading is.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use super::day_at;
use crate::baseline::Date;

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

#[test]
fn a_clock_reading_is_the_utc_day_it_falls_in() {
    assert_eq!(day_at(Duration::ZERO), Some(day("1970-01-01")));
    assert_eq!(day_at(Duration::from_secs(86_399)), Some(day("1970-01-01")));
    assert_eq!(day_at(Duration::from_secs(86_400)), Some(day("1970-01-02")));
    // 2026-10-25, at its first second, mid-morning and its last.
    for second in [0, 37_000, 86_399] {
        assert_eq!(
            day_at(Duration::from_secs(20_751 * 86_400 + second)),
            Some(day("2026-10-25")),
            "{second}"
        );
    }
    assert_eq!(
        day_at(Duration::from_secs(2_932_897 * 86_400)),
        None,
        "the year 10000 is no date a baseline can name"
    );
    assert_eq!(day_at(Duration::MAX), None);
}
