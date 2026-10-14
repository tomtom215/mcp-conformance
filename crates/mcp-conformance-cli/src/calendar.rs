// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The proleptic Gregorian calendar as days since 1970-01-01, both ways
//! (Howard Hinnant's algorithms, public domain).
//!
//! One file, compiled into the library (the importers' timestamps) and the
//! binary (the workbench's clock), so the conversion is written, and tested,
//! once.

// Private in both crates, so `pub(crate)` is the reach meant; `unreachable_pub`
// (rustc) and `redundant_pub_crate` (clippy nursery) make opposite demands of
// it, and this follows the rustc lint, as `main.rs` does.
#![allow(clippy::redundant_pub_crate)]

/// The days since 1970-01-01 of `year`-`month`-`day` (`month` 1–12, `day`
/// 1–31; a day past the month's end runs into the next).
pub(crate) const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The (year, month, day) of `days` since 1970-01-01.
pub(crate) const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (year_of_era + era * 400 + (month <= 2) as i64, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every day of four whole 400-year cycles, two before 1970 and two from
    /// it, so the era-boundary terms in `civil_from_days` (day 36 524 of an
    /// era, and its last, 146 096) and the negative eras are reached: each day
    /// is the one after the previous, by month lengths computed here, and
    /// round-trips through `days_from_civil`.
    #[test]
    fn four_whole_eras_step_one_calendar_day_at_a_time() {
        let leap = |year: i64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let length = |year: i64, month: i64| match month {
            2 if leap(year) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        // 1170-01-01 is 2 × 146 097 days before 1970-01-01: an era is exactly
        // 400 years.
        let mut expected = (1170, 1, 1);
        for days in -2 * 146_097..2 * 146_097_i64 {
            let date = civil_from_days(days);
            assert_eq!(date, expected, "day {days}");
            let (year, month, day) = date;
            assert_eq!(days_from_civil(year, month, day), days, "{date:?}");
            expected = if day < length(year, month) {
                (year, month, day + 1)
            } else if month < 12 {
                (year, month + 1, 1)
            } else {
                (year + 1, 1, 1)
            };
        }
        assert_eq!(expected, (2770, 1, 1));
    }
}
