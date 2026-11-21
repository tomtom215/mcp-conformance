// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A calendar date (UTC), for a baseline entry's `expires`.
//!
//! Written here rather than taken from a date crate: the baseline needs one
//! comparison and one conversion, and the engine reads no clock — the caller
//! hands [`Baseline::apply`](super::Baseline::apply) today's date.

use core::fmt;
use core::str::FromStr;

/// A proleptic Gregorian date, years 1 to 9999.
///
/// Ordered chronologically (year, then month, then day), which is the order
/// of its `YYYY-MM-DD` text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

/// Why a string is not a [`Date`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DateError {
    text: String,
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a calendar date written YYYY-MM-DD",
            self.text
        )
    }
}

impl core::error::Error for DateError {}

const fn is_leap(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

impl Date {
    /// The date of `year`-`month`-`day`, when it exists.
    ///
    /// ```
    /// use mcp_trace_validator::baseline::Date;
    ///
    /// assert!(Date::new(2028, 2, 29).is_some(), "2028 is a leap year");
    /// assert!(Date::new(2027, 2, 29).is_none());
    /// ```
    #[must_use]
    pub const fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if year == 0 || year > 9999 || month == 0 || month > 12 {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// The date `days` days after 1970-01-01 (before it, when negative), for a
    /// caller that has read a clock: `seconds / 86_400`. `None` outside years
    /// 1 to 9999.
    ///
    /// ```
    /// use mcp_trace_validator::baseline::Date;
    ///
    /// assert_eq!(Date::from_unix_days(0).unwrap().to_string(), "1970-01-01");
    /// assert_eq!(Date::from_unix_days(20_751).unwrap().to_string(), "2026-10-25");
    /// ```
    #[must_use]
    pub fn from_unix_days(days: i64) -> Option<Self> {
        // Howard Hinnant's `civil_from_days`, over the era of 400 years.
        let shifted = days.checked_add(719_468)?;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self::new(
            u16::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
        )
    }
}

impl FromStr for Date {
    type Err = DateError;

    /// Exactly `YYYY-MM-DD`: four digits, two, two, no sign and no spaces,
    /// naming a day that exists.
    fn from_str(text: &str) -> Result<Self, DateError> {
        let bad = || DateError {
            text: text.to_owned(),
        };
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
        if !shaped {
            return Err(bad());
        }
        let number = |range: core::ops::Range<usize>| text[range].parse::<u16>().map_err(|_| bad());
        let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
        let month = u8::try_from(month).map_err(|_| bad())?;
        let day = u8::try_from(day).map_err(|_| bad())?;
        Self::new(year, month, day).ok_or_else(bad)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}
