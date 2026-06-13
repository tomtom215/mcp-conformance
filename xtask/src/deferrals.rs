// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The deferral ledger gate: consciously deferred work expires loudly.
//!
//! `docs/plan/deferrals.json` is the ledger (ADR-0010): every deferred
//! capability carries what/why/meanwhile and a `review_by` date. This task
//! lists the ledger; `--check` (run by the weekly scheduled job, never the
//! PR gate — an expiry must not block unrelated work, it must page the
//! schedule) fails once any row passes its date without being re-decided.
//! Re-deciding means building the thing (delete the row) or re-dating it
//! with a fresh reason in the same commit — prose alone never expires.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use serde::Deserialize;

/// The committed ledger, relative to the workspace root.
const LEDGER: &str = "docs/plan/deferrals.json";

/// One deferred piece of work. Unknown fields are rejected so a typo cannot
/// invent an unenforced field.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deferral {
    id: String,
    what: String,
    #[allow(dead_code)]
    why: String,
    #[allow(dead_code)]
    meanwhile: String,
    /// ISO date (`YYYY-MM-DD`); lexicographic comparison is date comparison.
    review_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    #[serde(rename = "_policy")]
    #[allow(dead_code)]
    policy: String,
    deferrals: Vec<Deferral>,
}

/// Runs the task; `check` makes expired rows fail it.
pub(crate) fn run(check: bool) -> bool {
    let path = crate::workspace_root().join(LEDGER);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask: deferrals — cannot read {}: {error}", path.display());
            return false;
        }
    };
    let ledger: Ledger = match serde_json::from_str(&text) {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!(
                "xtask: deferrals — {} is not valid: {error}",
                path.display()
            );
            return false;
        }
    };
    let today = today_utc();
    let mut expired = Vec::new();
    for row in &ledger.deferrals {
        if !valid_iso_date(&row.review_by) {
            eprintln!(
                "xtask: deferrals — {} has review_by {:?}; dates are YYYY-MM-DD",
                row.id, row.review_by
            );
            return false;
        }
        let state = if row.review_by.as_str() < today.as_str() {
            expired.push(row);
            "EXPIRED"
        } else {
            "open"
        };
        eprintln!(
            "xtask: deferrals — {:9} {} (review by {}): {}",
            state, row.id, row.review_by, row.what
        );
    }
    if check && !expired.is_empty() {
        eprintln!(
            "xtask: deferrals — {} row(s) passed review_by without being re-decided. \
             Build the deferred thing and delete its row, or re-date it with a fresh \
             reason in the same commit ({LEDGER}).",
            expired.len()
        );
        return false;
    }
    true
}

/// Today as `YYYY-MM-DD` (UTC), via the proleptic-Gregorian civil-from-days
/// algorithm (Howard Hinnant) — no calendar dependency for one date.
fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Civil date from days since 1970-01-01 (valid for the era this gate runs in).
const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let shifted = z + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// `YYYY-MM-DD` shape with sane ranges (not a full calendar validation —
/// the ledger is reviewed code, not hostile input).
fn valid_iso_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && date
            .split('-')
            .enumerate()
            .all(|(index, part)| match (index, part.parse::<u32>()) {
                (0, Ok(year)) => (2020..=9999).contains(&year),
                (1, Ok(month)) => (1..=12).contains(&month),
                (2, Ok(day)) => (1..=31).contains(&day),
                _ => false,
            })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // leap year start
        assert_eq!(civil_from_days(20_617), (2026, 6, 13));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29)); // leap day
    }

    #[test]
    fn iso_date_shape_is_enforced() {
        assert!(valid_iso_date("2026-08-15"));
        assert!(!valid_iso_date("2026-8-15"));
        assert!(!valid_iso_date("2026-13-01"));
        assert!(!valid_iso_date("soon"));
        assert!(!valid_iso_date("2026-08-15T00:00:00Z"));
    }

    #[test]
    fn today_is_an_iso_date_after_the_project_began() {
        let today = today_utc();
        assert!(valid_iso_date(&today), "{today}");
        assert!(today.as_str() >= "2026-06-12", "{today}");
    }

    #[test]
    fn the_committed_ledger_parses_and_dates_are_well_formed() {
        // The gate's own input contract, pinned: the committed ledger must
        // always parse strictly, whatever today's date is.
        let path = crate::workspace_root().join(LEDGER);
        let text = std::fs::read_to_string(&path).unwrap();
        let ledger: Ledger = serde_json::from_str(&text).unwrap();
        assert!(!ledger.deferrals.is_empty());
        for row in &ledger.deferrals {
            assert!(
                valid_iso_date(&row.review_by),
                "{}: {}",
                row.id,
                row.review_by
            );
        }
    }
}
