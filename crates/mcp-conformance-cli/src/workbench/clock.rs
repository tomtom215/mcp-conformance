// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Wall-clock time as the library records it: RFC 3339 in UTC, to the
//! second, and the calendar day a recording belongs to.
//!
//! Written here rather than taken from a date crate: the library needs "now"
//! as text, a day from a timestamp's first ten characters, and the age of a
//! day in whole days for retention — the civil-from-days conversion below
//! (Howard Hinnant's, public domain) is the whole of it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds in a day.
const DAY: u64 = 86_400;

/// `time` as `YYYY-MM-DDTHH:MM:SSZ`; the epoch for a time before it.
pub(crate) fn rfc3339(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let (year, month, day) = civil(seconds / DAY);
    let rest = seconds % DAY;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// Now, as [`rfc3339`] writes it.
pub(crate) fn now() -> String {
    rfc3339(SystemTime::now())
}

/// The calendar day (`YYYY-MM-DD`) an RFC 3339 timestamp names, when it
/// starts with one.
pub(crate) fn day(timestamp: &str) -> Option<&str> {
    let day = timestamp.get(..10)?;
    days_from_civil(day).map(|_| day)
}

/// Whole days from `day` (`YYYY-MM-DD`) to `today` (the same form); `None`
/// when either is not a date.
pub(crate) fn age_in_days(day: &str, today: &str) -> Option<i64> {
    Some(days_from_civil(today)? - days_from_civil(day)?)
}

/// The days since 1970-01-01 of `day` (`YYYY-MM-DD`).
fn days_from_civil(day: &str) -> Option<i64> {
    let bytes = day.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let number = |range: core::ops::Range<usize>| -> Option<i64> {
        let text = day.get(range)?;
        text.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| text.parse().ok())?
    };
    let (year, month, date) = (number(0..4)?, number(5..7)?, number(8..10)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&date) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + date - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

/// The (year, month, day) of `days` since 1970-01-01.
fn civil(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let date = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_in_utc_to_the_second() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400 + 3_723);
        assert_eq!(rfc3339(leap), "2000-02-29T01:02:03Z");
        let later = UNIX_EPOCH + Duration::from_secs(1_791_936_000);
        assert_eq!(rfc3339(later), "2026-10-14T00:00:00Z");
        assert_eq!(
            rfc3339(UNIX_EPOCH - Duration::from_secs(5)),
            "1970-01-01T00:00:00Z"
        );
        assert!(now().ends_with('Z'));
    }

    #[test]
    fn days_round_trip_through_the_calendar() {
        for days in [0_u64, 59, 60, 365, 10_957, 11_016, 20_740, 2_932_896] {
            let (year, month, date) = civil(days);
            let text = format!("{year:04}-{month:02}-{date:02}");
            assert_eq!(days_from_civil(&text), i64::try_from(days).ok(), "{text}");
        }
    }

    /// Every day of two whole 400-year cycles, so the era-boundary terms in
    /// `civil` (day 36 524 of an era, and its last, 146 096) are reached: each
    /// day is the one after the previous, by month lengths computed here, and
    /// round-trips through `days_from_civil`.
    #[test]
    fn two_whole_eras_step_one_calendar_day_at_a_time() {
        let leap = |year: u64| {
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
        };
        let length = |year: u64, month: u64| match month {
            2 if leap(year) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        let mut expected = (1970, 1, 1);
        for days in 0..2 * 146_097_u64 {
            let date = civil(days);
            assert_eq!(date, expected, "day {days}");
            let text = format!("{:04}-{:02}-{:02}", date.0, date.1, date.2);
            assert_eq!(days_from_civil(&text), i64::try_from(days).ok(), "{text}");
            let (year, month, day) = date;
            expected = if day < length(year, month) {
                (year, month, day + 1)
            } else if month < 12 {
                (year, month + 1, 1)
            } else {
                (year + 1, 1, 1)
            };
        }
    }

    #[test]
    fn a_day_is_read_only_from_a_date() {
        assert_eq!(day("2026-10-13T10:54:29.018Z"), Some("2026-10-13"));
        assert_eq!(day("2026-10-13"), Some("2026-10-13"));
        for bad in [
            "",
            "2026-10",
            "2026/10/13",
            "2026-13-01",
            "2026-00-10",
            "2026-10-32",
            "20x6-10-13",
            "+026-10-13",
        ] {
            assert_eq!(day(bad), None, "{bad}");
        }
    }

    #[test]
    fn ages_are_whole_days() {
        assert_eq!(age_in_days("2026-10-01", "2026-10-13"), Some(12));
        assert_eq!(age_in_days("2026-02-28", "2026-03-01"), Some(1));
        assert_eq!(age_in_days("2024-02-28", "2024-03-01"), Some(2));
        assert_eq!(age_in_days("2026-10-14", "2026-10-13"), Some(-1));
        assert_eq!(age_in_days("nope", "2026-10-13"), None);
        assert_eq!(age_in_days("2026-10-13", "nope"), None);
    }
}
