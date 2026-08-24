// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The register-currency gate: the register's 90-day rule, read from the
//! register's own dates.
//!
//! `docs/plan/01-ecosystem-context.md` states maintenance rule 1 in its own
//! prose — *"a row older than 90 days must be re-verified before it is cited in
//! anything external"* — and until [ADR-0015] nothing read it. The rule
//! depended on someone hand-writing a deferral-ledger row that scheduled the
//! sweep, and that row's own scope figure was a `grep` miscount. A rule stated
//! in prose beside data that already carries the dates is a rule waiting to be
//! forgotten.
//!
//! Two halves, split by who caused the failure and who can fix it:
//!
//! - [`structure_gate`] runs inside `cargo xtask ci`. It checks the shape a
//!   row must have to be readable at all — a status the register defines, a
//!   date that parses, no date in the future. A malformed row is a defect in
//!   the change that wrote it, so it fails that change.
//! - [`currency_gate`] runs weekly in the `claims-expire` job. It fails once
//!   any row passes ninety days. That is [ADR-0010]'s division exactly: an
//!   expiry pages the schedule, it does not block a pull request that has
//!   nothing to do with it.
//!
//! [ADR-0010]: ../../../docs/plan/decisions/0010-deferral-ledger-and-scheduled-reverification.md
//! [ADR-0015]: ../../../docs/plan/decisions/0015-the-tier-2-premise-is-gone.md

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

/// The register, relative to the workspace root.
const REGISTER: &str = "docs/plan/01-ecosystem-context.md";

/// Rule 1's window, in days.
const MAX_AGE_DAYS: i64 = 90;

/// The three statuses the register's own preamble defines. A fourth would be a
/// status nobody has told a reader how to interpret.
const STATUSES: [&str; 3] = ["Verified", "Partial", "Unverified"];

/// A row that survived parsing: which fact, when it was last verified, and
/// where to find it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The row's `#` cell — `2.8`, `1.5i`, and so on.
    id: String,
    /// Line number in the register, for messages that can be jumped to.
    line: usize,
    /// `YYYY-MM-DD`, from the row's own `Verified` cell or, for a table with no
    /// such column, from its header's currency date.
    verified: String,
}

/// Whether `line` is a table row rather than prose or a separator.
fn cells(line: &str) -> Option<Vec<&str>> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(str::trim).collect())
}

/// The first `YYYY-MM-DD` in `text`, if there is one.
///
/// Used two ways, and the "first" matters for both. A `Verified` cell may carry
/// prose after its date (`2026-06-09 (2026-08-24 re-verification blocked)`), and
/// the currency date of a dateless table sits in a header cell that also records
/// the previous reading (`Scale (2026-08-24; was 2026-06-09)`). In both, the
/// leading date is the current one; a scan for the *oldest* date would read the
/// history and report the row as stale forever.
///
/// Scans by char index, and takes the window with `get` rather than `[..]`. An
/// earlier version indexed bytes and sliced directly, which panicked on the
/// first multi-byte character before a date — `"re-checked — 2026-08-24"` aborts
/// with *end byte index 12 is not a char boundary*. It passed on the real
/// register only because every date there happens to sit at byte zero of its
/// cell or behind pure ASCII, so the first row written with an em-dash in front
/// of its date would have crashed the gate instead of failing it. A gate that
/// panics reports nothing about the thing it was asked to check.
fn first_date(text: &str) -> Option<String> {
    text.char_indices()
        .find_map(|(start, _)| {
            text.get(start..start.checked_add(10)?)
                .filter(|candidate| looks_like_iso_date(candidate))
        })
        .map(str::to_owned)
}

/// `YYYY-MM-DD` shape with sane ranges — the same bar `deferrals` applies, for
/// the same reason: this is reviewed prose, not hostile input.
fn looks_like_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && text
            .split('-')
            .enumerate()
            .all(|(index, part)| match (index, part.parse::<u32>()) {
                (0, Ok(year)) => (2020..=9999).contains(&year),
                (1, Ok(month)) => (1..=12).contains(&month),
                (2, Ok(day)) => (1..=31).contains(&day),
                _ => false,
            })
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` that has already passed
/// [`looks_like_iso_date`] — the inverse of `deferrals::civil_from_days`
/// (Howard Hinnant's `days_from_civil`), so an age is a subtraction rather than
/// a calendar dependency.
fn days_from_civil(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

/// Every row the register carries, or the first thing wrong with the file.
///
/// Split out from the gates so both share one reading and the rules are
/// testable against a string rather than the tree.
pub(crate) fn parse(markdown: &str, today: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    let mut header: Option<(Vec<String>, Option<String>)> = None;
    for (index, line) in markdown.lines().enumerate() {
        let number = index + 1;
        let Some(cells) = cells(line) else {
            continue;
        };
        if cells.first().is_some_and(|first| *first == "#") {
            let currency = cells.iter().find_map(|cell| first_date(cell));
            header = Some((
                cells.iter().map(|cell| (*cell).to_owned()).collect(),
                currency,
            ));
            continue;
        }
        if cells
            .first()
            .is_some_and(|first| first.chars().all(|byte| matches!(byte, '-' | ':' | ' ')))
        {
            continue;
        }
        let Some((columns, currency)) = header.as_ref() else {
            continue;
        };
        if cells.len() != columns.len() {
            return Err(format!(
                "line {number}: row {:?} has {} cells, its table's header has {}. A row whose \
                 columns do not line up is not readable as a fact",
                cells.first().unwrap_or(&""),
                cells.len(),
                columns.len()
            ));
        }
        let row = read_row(&cells, columns, currency.as_deref(), number)?;
        if row.verified.as_str() > today {
            return Err(format!(
                "line {number}: row {} is verified {} — in the future. A date nobody could have \
                 fetched a source on is worse than no date",
                row.id, row.verified
            ));
        }
        rows.push(row);
    }
    if rows.len() < 40 {
        return Err(format!(
            "only {} rows parsed out of {REGISTER}; the register has carried more than sixty for \
             months, so this is a parser that stopped seeing the table rather than a register \
             that shrank",
            rows.len()
        ));
    }
    Ok(rows)
}

/// One row's status and date, given its table's columns.
fn read_row(
    cells: &[&str],
    columns: &[String],
    currency: Option<&str>,
    number: usize,
) -> Result<Row, String> {
    let id = (*cells.first().unwrap_or(&"")).to_owned();
    let column = |name: &str| columns.iter().position(|heading| heading == name);
    if let Some(index) = column("Status") {
        let status = cells[index];
        if !STATUSES.contains(&status) {
            return Err(format!(
                "line {number}: row {id} has status {status:?}; the register's preamble defines \
                 exactly {STATUSES:?}"
            ));
        }
    }
    let verified = match column("Verified") {
        Some(index) => first_date(cells[index]).ok_or_else(|| {
            format!(
                "line {number}: row {id} has Verified {:?}, which starts with no YYYY-MM-DD date. \
                 Trailing prose is fine; a leading date is not optional",
                cells[index]
            )
        })?,
        None => currency
            .ok_or_else(|| {
                format!(
                    "line {number}: row {id} is in a table with no Verified column, and that \
                     table's header carries no YYYY-MM-DD currency date either. Every fact in \
                     this register is dated, per-row or per-table"
                )
            })?
            .to_owned(),
    };
    Ok(Row {
        id,
        line: number,
        verified,
    })
}

/// The rows past `MAX_AGE_DAYS`, oldest first, with each age in days.
fn stale(rows: &[Row], today: &str) -> Vec<(usize, i64)> {
    let Some(now) = days_from_civil(today) else {
        return Vec::new();
    };
    let mut out: Vec<(usize, i64)> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let age = now - days_from_civil(&row.verified)?;
            (age > MAX_AGE_DAYS).then_some((index, age))
        })
        .collect();
    out.sort_by_key(|(index, age)| (-age, *index));
    out
}

/// Reads the register, or explains why it could not.
fn load() -> Result<(String, String), String> {
    let path = crate::workspace_root().join(REGISTER);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok((text, crate::deferrals::today_utc()))
}

/// The PR half: every row is shaped so a reader — and this gate — can date it.
pub(crate) fn structure_gate() -> bool {
    let (text, today) = match load() {
        Ok(pair) => pair,
        Err(complaint) => {
            eprintln!("xtask: register-currency — {complaint}");
            return false;
        }
    };
    match parse(&text, &today) {
        Ok(rows) => {
            eprintln!(
                "xtask: register-currency — {} dated rows, all statuses and dates well-formed \
                 ({REGISTER})",
                rows.len()
            );
            true
        }
        Err(complaint) => {
            eprintln!("xtask: register-currency — {complaint}");
            false
        }
    }
}

/// The weekly half: rule 1, enforced.
pub(crate) fn currency_gate() -> bool {
    let (text, today) = match load() {
        Ok(pair) => pair,
        Err(complaint) => {
            eprintln!("xtask: register-currency — {complaint}");
            return false;
        }
    };
    let rows = match parse(&text, &today) {
        Ok(rows) => rows,
        Err(complaint) => {
            eprintln!("xtask: register-currency — {complaint}");
            return false;
        }
    };
    let stale = stale(&rows, &today);
    if stale.is_empty() {
        eprintln!(
            "xtask: register-currency — all {} rows re-verified within {MAX_AGE_DAYS} days",
            rows.len()
        );
        return true;
    }
    for (index, age) in &stale {
        let row = &rows[*index];
        eprintln!(
            "xtask: register-currency — STALE {} ({REGISTER}:{}) verified {}, {age} days ago",
            row.id, row.line, row.verified
        );
    }
    eprintln!(
        "xtask: register-currency — {} row(s) past the register's own 90-day rule. Re-verify each \
         against its listed primary source and update value, date and source in place \
         (maintenance rule 2); if a source cannot be reached, say so in the row and leave its \
         date alone rather than advancing it.",
        stale.len()
    );
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const HEADER: &str = "| # | Fact | Status | Verified | Source | Used by |\n\
                          |---|------|--------|----------|--------|---------|\n";

    fn synthetic(rows: usize, date: &str) -> String {
        use std::fmt::Write as _;
        let mut text = String::from(HEADER);
        for index in 0..rows {
            let _ = writeln!(
                text,
                "| {index}.0 | a fact | Verified | {date} | a source | a consumer |"
            );
        }
        text
    }

    #[test]
    fn days_from_civil_inverts_civil_from_days() {
        for day in [0_i64, 11_016, 19_723, 20_617, 20_688] {
            let (year, month, day_of_month) = crate::deferrals::civil_from_days(day);
            let date = format!("{year:04}-{month:02}-{day_of_month:02}");
            assert_eq!(days_from_civil(&date), Some(day), "round trip for {date}");
        }
    }

    #[test]
    fn a_verified_cell_may_carry_prose_after_its_date() {
        // Real rows do: 1.5i and the rows the 2026-08-24 sweep could not reach
        // both append an explanation. The date leads; the prose is for humans.
        assert_eq!(
            first_date("2026-06-09 (2026-08-24 re-verification blocked — see fact)").unwrap(),
            "2026-06-09"
        );
        assert_eq!(
            first_date("2026-08-17 (runner-distinguishability re-measured 2026-08-18)").unwrap(),
            "2026-08-17"
        );
        assert_eq!(
            first_date("Scale (2026-08-24; was 2026-06-09)").unwrap(),
            "2026-08-24"
        );
        assert!(first_date("no date here at all").is_none());
    }

    #[test]
    fn a_date_behind_a_multibyte_character_is_found_rather_than_panicking() {
        // The register is full of em-dashes. This scanned bytes and sliced
        // directly until 2026-08-24, which aborts with "end byte index 12 is
        // not a char boundary" — reproduced before the fix, not theorised.
        assert_eq!(first_date("re-checked — 2026-08-24").unwrap(), "2026-08-24");
        assert_eq!(
            first_date("héllo… 2026-01-02 — see fact").unwrap(),
            "2026-01-02"
        );
        // Multi-byte characters and no date at all: still None, still no panic.
        assert!(first_date("— … é ✓ no date").is_none());
    }

    #[test]
    fn a_row_dated_in_the_future_is_rejected() {
        let text = synthetic(45, "2026-09-01");
        let complaint = parse(&text, "2026-08-24").unwrap_err();
        assert!(complaint.contains("in the future"), "{complaint}");
    }

    #[test]
    fn a_status_outside_the_three_is_rejected() {
        let text = synthetic(45, "2026-08-24").replace("| Verified | 2026", "| Probably | 2026");
        let complaint = parse(&text, "2026-08-24").unwrap_err();
        assert!(complaint.contains("preamble defines"), "{complaint}");
    }

    #[test]
    fn a_row_whose_columns_do_not_line_up_is_rejected() {
        let mut text = synthetic(45, "2026-08-24");
        text.push_str("| 99.0 | a fact | Verified | 2026-08-24 |\n");
        let complaint = parse(&text, "2026-08-24").unwrap_err();
        assert!(complaint.contains("do not line up"), "{complaint}");
    }

    #[test]
    fn a_table_without_a_verified_column_takes_its_headers_date() {
        let text = format!(
            "{}| # | Tool | Scale (2026-08-24; was 2026-06-09) |\n|---|---|---|\n\
             | 5.1 | a tool | some downloads |\n",
            synthetic(45, "2026-08-24")
        );
        let rows = parse(&text, "2026-08-24").unwrap();
        let five = rows.iter().find(|row| row.id == "5.1").unwrap();
        assert_eq!(five.verified, "2026-08-24");
    }

    #[test]
    fn a_dateless_table_without_a_dateless_header_is_rejected() {
        let text = format!(
            "{}| # | Tool | Scale |\n|---|---|---|\n| 5.1 | a tool | some downloads |\n",
            synthetic(45, "2026-08-24")
        );
        let complaint = parse(&text, "2026-08-24").unwrap_err();
        assert!(complaint.contains("currency date"), "{complaint}");
    }

    #[test]
    fn a_parser_that_stops_seeing_rows_fails_rather_than_passing_vacuously() {
        // The failure this guards is the one every walk-the-tree gate has: a
        // reshaped table yields zero rows, `stale` is empty, and the gate
        // reports success having checked nothing.
        let complaint = parse(&synthetic(3, "2026-08-24"), "2026-08-24").unwrap_err();
        assert!(
            complaint.contains("stopped seeing the table"),
            "{complaint}"
        );
    }

    #[test]
    fn ninety_days_is_the_boundary_and_it_is_exclusive() {
        let rows = parse(&synthetic(45, "2026-06-09"), "2026-09-07").unwrap();
        assert!(
            stale(&rows, "2026-09-07").is_empty(),
            "2026-06-09 is exactly 90 days before 2026-09-07 and must not be stale yet"
        );
        let rows = parse(&synthetic(45, "2026-06-09"), "2026-09-08").unwrap();
        assert_eq!(
            stale(&rows, "2026-09-08").len(),
            45,
            "one day later, all of them"
        );
    }

    #[test]
    fn the_real_register_parses_and_is_current_today() {
        let (text, today) = load().unwrap();
        let rows = parse(&text, &today).unwrap();
        assert!(
            rows.len() >= 68,
            "register shrank unexpectedly: {}",
            rows.len()
        );
        let stale = stale(&rows, &today);
        assert!(
            stale.is_empty(),
            "rows past the 90-day rule: {:?}",
            stale
                .iter()
                .map(|(index, age)| (&rows[*index].id, age))
                .collect::<Vec<_>>()
        );
    }
}
