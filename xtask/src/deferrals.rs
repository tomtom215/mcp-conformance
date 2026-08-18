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
//!
//! `--expired` is the same decision asked as a question rather than as a
//! gate: it prints `id review_by` for each expired row on stdout and exits
//! zero. The scheduled job's notification step formats those lines into the
//! tracking issue it opens, so the issue names the rows from the committed
//! ledger instead of scraping a log whose shape nothing pins.

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

/// What one run of the task is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// List every row on stderr; dates are reported, never enforced.
    List,
    /// List every row and fail if any has passed its `review_by` — the gate
    /// the weekly scheduled job runs.
    Check,
    /// Print `id review_by` for each expired row on stdout and nothing else.
    /// A query, not a gate: it exits zero however many rows it names.
    Expired,
}

impl Mode {
    /// Parses the task's one optional flag.
    ///
    /// Unknown flags are rejected rather than silently taken as the default:
    /// `deferrals --chekc` used to list the ledger and exit zero, which is
    /// indistinguishable from a gate that ran and passed.
    fn parse(flag: Option<&str>) -> Option<Self> {
        match flag {
            None => Some(Self::List),
            Some("--check") => Some(Self::Check),
            Some("--expired") => Some(Self::Expired),
            Some(_) => None,
        }
    }
}

/// Entry point from `main`: parses the flag, then runs that mode.
pub(crate) fn dispatch(flag: Option<&str>) -> bool {
    Mode::parse(flag).map_or_else(
        || {
            eprintln!(
                "xtask: deferrals — unknown flag {:?}; expected --check or --expired",
                flag.unwrap_or_default()
            );
            false
        },
        run,
    )
}

/// Runs the task in `mode`.
pub(crate) fn run(mode: Mode) -> bool {
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
    if let Some(complaint) = shape_complaint(&ledger) {
        eprintln!("xtask: deferrals — {complaint}");
        return false;
    }
    let today = today_utc();
    let expired = expired(&ledger, &today);
    if mode == Mode::Expired {
        for row in &expired {
            println!("{}", expired_line(row));
        }
        return true;
    }
    for row in &ledger.deferrals {
        let state = if is_expired(row, &today) {
            "EXPIRED"
        } else {
            "open"
        };
        eprintln!(
            "xtask: deferrals — {:9} {} (review by {}): {}",
            state, row.id, row.review_by, row.what
        );
    }
    if mode == Mode::Check && !expired.is_empty() {
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

/// Whether `row`'s review date has passed — the gate's whole decision, on one
/// row. Both dates are `YYYY-MM-DD`, where lexicographic order is date order.
fn is_expired(row: &Deferral, today: &str) -> bool {
    row.review_by.as_str() < today
}

/// The expired rows, in ledger order.
fn expired<'a>(ledger: &'a Ledger, today: &str) -> Vec<&'a Deferral> {
    ledger
        .deferrals
        .iter()
        .filter(|row| is_expired(row, today))
        .collect()
}

/// One line of `--expired` output.
///
/// Whitespace-separated because the consumer is `read -r id review_by` in the
/// scheduled job, and both fields are shape-checked above to contain no
/// whitespace. The row's `what` prose deliberately does not appear: this line
/// is copied into a GitHub issue body, and an id plus a date says which rows
/// to re-decide without republishing paragraphs of committed prose there.
fn expired_line(row: &Deferral) -> String {
    format!("{} {}", row.id, row.review_by)
}

/// The first thing wrong with the ledger's row shapes, if anything is.
///
/// Separate from `run` so the rules are testable without a ledger on disk.
fn shape_complaint(ledger: &Ledger) -> Option<String> {
    let mut seen = std::collections::BTreeSet::new();
    for row in &ledger.deferrals {
        if !valid_id(&row.id) {
            return Some(format!(
                "{:?} is not a usable id; ids are lowercase ASCII words joined by \
                 single hyphens (they are printed whitespace-separated by --expired)",
                row.id
            ));
        }
        if !seen.insert(row.id.as_str()) {
            return Some(format!(
                "{} appears twice; two rows sharing an id cannot be re-decided \
                 separately",
                row.id
            ));
        }
        if !valid_iso_date(&row.review_by) {
            return Some(format!(
                "{} has review_by {:?}; dates are YYYY-MM-DD",
                row.id, row.review_by
            ));
        }
    }
    None
}

/// `lowercase-ascii-words-joined-by-hyphens`, non-empty, no doubled or edge
/// hyphens.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
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

    /// A ledger of `(id, review_by)` rows, built through serde so the tests
    /// exercise the same strict deserialization the gate does.
    fn ledger_of(rows: &[(&str, &str)]) -> Ledger {
        let deferrals: Vec<_> = rows
            .iter()
            .map(|(id, review_by)| {
                serde_json::json!({
                    "id": id,
                    "what": "a thing",
                    "why": "a reason",
                    "meanwhile": "an enforcement",
                    "review_by": review_by,
                })
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "_policy": "p",
            "deferrals": deferrals,
        }))
        .unwrap()
    }

    #[test]
    fn every_flag_the_usage_advertises_parses_and_nothing_else_does() {
        assert_eq!(Mode::parse(None), Some(Mode::List));
        assert_eq!(Mode::parse(Some("--check")), Some(Mode::Check));
        assert_eq!(Mode::parse(Some("--expired")), Some(Mode::Expired));
        // The whole point: a typo is a refusal, not a silent pass.
        assert_eq!(Mode::parse(Some("--chekc")), None);
        assert_eq!(Mode::parse(Some("check")), None);
        assert_eq!(Mode::parse(Some("")), None);
        assert!(!dispatch(Some("--chekc")));
    }

    #[test]
    fn expiry_is_strictly_before_today_so_review_day_is_still_open() {
        let ledger = ledger_of(&[
            ("yesterday", "2026-08-17"),
            ("today", "2026-08-18"),
            ("tomorrow", "2026-08-19"),
        ]);
        let expired = expired(&ledger, "2026-08-18");
        let ids: Vec<&str> = expired.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["yesterday"]);
    }

    #[test]
    fn an_expired_line_is_the_id_and_the_date_the_shell_reads() {
        let ledger = ledger_of(&[("register-90-day-sweep", "2026-09-07")]);
        assert_eq!(
            expired_line(&ledger.deferrals[0]),
            "register-90-day-sweep 2026-09-07"
        );
        // `read -r id review_by` splits on whitespace: exactly two fields.
        assert_eq!(
            expired_line(&ledger.deferrals[0])
                .split_whitespace()
                .count(),
            2
        );
    }

    #[test]
    fn ids_that_would_not_survive_the_shell_or_a_re_decision_are_refused() {
        assert!(valid_id("draft-suite-pin-currency"));
        assert!(valid_id("rust-sdk-902-offer-clock"));
        assert!(!valid_id(""));
        assert!(!valid_id("-leading"));
        assert!(!valid_id("trailing-"));
        assert!(!valid_id("double--hyphen"));
        assert!(!valid_id("Has Spaces"));
        assert!(!valid_id("UPPER"));
        assert!(!valid_id("under_score"));
    }

    #[test]
    fn shape_complaints_name_the_row_and_the_rule() {
        assert!(shape_complaint(&ledger_of(&[("fine", "2026-09-07")])).is_none());

        let bad_id = shape_complaint(&ledger_of(&[("Not An Id", "2026-09-07")])).unwrap();
        assert!(bad_id.contains("Not An Id"), "{bad_id}");

        let duplicate = shape_complaint(&ledger_of(&[
            ("same", "2026-09-07"),
            ("same", "2026-10-07"),
        ]))
        .unwrap();
        assert!(duplicate.contains("twice"), "{duplicate}");

        let bad_date = shape_complaint(&ledger_of(&[("fine", "soon")])).unwrap();
        assert!(bad_date.contains("YYYY-MM-DD"), "{bad_date}");
    }

    #[test]
    fn the_committed_ledger_parses_and_dates_are_well_formed() {
        // The gate's own input contract, pinned: the committed ledger must
        // always parse strictly, whatever today's date is.
        let path = crate::workspace_root().join(LEDGER);
        let text = std::fs::read_to_string(&path).unwrap();
        let ledger: Ledger = serde_json::from_str(&text).unwrap();
        assert!(!ledger.deferrals.is_empty());
        assert_eq!(shape_complaint(&ledger), None);
    }
}
