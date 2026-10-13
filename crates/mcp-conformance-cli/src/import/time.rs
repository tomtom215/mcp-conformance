// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! RFC 3339 UTC timestamps as milliseconds since 1970, and back: what the
//! Inspector's export needs to place a response by its request's timestamp
//! and the duration it recorded. JavaScript's `Date.prototype.toISOString`
//! writes `YYYY-MM-DDTHH:MM:SS.sssZ`, which is the form read here; anything
//! else is not a time this module claims to read.

/// The milliseconds since 1970 of a `YYYY-MM-DDTHH:MM:SS[.fff…]Z` timestamp.
pub(super) fn parse_millis(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[10] != b'T' || !text.ends_with('Z') {
        return None;
    }
    let digits = |range: core::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let fraction = &text[19..text.len() - 1];
    let millis = match fraction.strip_prefix('.') {
        None if fraction.is_empty() => 0,
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            let mut padded: String = digits.chars().take(3).collect();
            while padded.len() < 3 {
                padded.push('0');
            }
            padded.parse::<i64>().ok()?
        }
        _ => return None,
    };
    let days = days_from_civil(year, month, day);
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000 + millis)
}

/// `millis` since 1970 as `YYYY-MM-DDTHH:MM:SS.sssZ`.
pub(super) fn format_millis(millis: i64) -> String {
    let days = millis.div_euclid(86_400_000);
    let rest = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm, public domain).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

const fn civil_from_days(days: i64) -> (i64, i64, i64) {
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

    #[test]
    fn javascript_timestamps_round_trip() {
        for text in [
            "1970-01-01T00:00:00.000Z",
            "2000-02-29T23:59:59.999Z",
            "2026-10-13T10:54:29.018Z",
            "1969-12-31T23:59:59.500Z",
        ] {
            let millis = parse_millis(text).unwrap_or(i64::MIN);
            assert_eq!(format_millis(millis), text);
        }
        assert_eq!(parse_millis("1970-01-01T00:00:01Z"), Some(1000));
        assert_eq!(parse_millis("1970-01-01T00:00:00.5Z"), Some(500));
        assert_eq!(parse_millis("1970-01-01T00:00:00.123456789Z"), Some(123));
    }

    #[test]
    fn other_forms_are_not_read() {
        for text in [
            "",
            "2026-10-13",
            "2026-10-13T10:54:29.018+02:00",
            "2026-10-13 10:54:29Z",
            "2026-13-13T10:54:29Z",
            "2026-10-13T24:00:00Z",
            "2026-10-13T10:54:29.Z",
            "2026-10-13T10:54:29.0x8Z",
            "2026/10/13T10:54:29Z",
            "2026-10-13T10-54:29Z",
            "x026-10-13T10:54:29Z",
        ] {
            assert_eq!(parse_millis(text), None, "{text}");
        }
    }
}
