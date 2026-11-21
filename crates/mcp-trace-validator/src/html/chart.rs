// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The summary's drawings: the counts as stacked bars, and the same counts
//! split by area of the protocol.
//!
//! Each bar is inline SVG whose geometry is attributes and whose colours are
//! classes of the stylesheet — no `style` attribute, nothing fetched — and it
//! is `aria-hidden`: the numbers it draws are always beside it as text.

use core::fmt::Write as _;

use super::Scope;
use super::clauses::{open_section, revision_heading};
use super::escape::Text;
use super::summary::plural;
use crate::report::{Outcome, Report, RequirementReport, Totals};

/// The width every bar is drawn in; the stylesheet stretches it to its box.
const WIDTH: u64 = 1000;

/// The widths of a bar's segments: proportional to `counts`, except that a
/// segment with anything in it is at least `floor` wide, so one failure among
/// hundreds of clauses can still be seen. Zero counts get zero width.
fn widths(counts: &[u32], floor: u64) -> Vec<u64> {
    let total: u64 = counts.iter().map(|count| u64::from(*count)).sum();
    let shown = counts.iter().filter(|count| **count > 0).count();
    let shown = u64::try_from(shown).unwrap_or(u64::MAX);
    if total == 0 {
        return vec![0; counts.len()];
    }
    let spare = WIDTH.saturating_sub(shown.saturating_mul(floor));
    let mut left = WIDTH;
    let mut seen = 0;
    counts
        .iter()
        .map(|count| {
            if *count == 0 {
                return 0;
            }
            seen += 1;
            // The last segment takes what rounding left over.
            let width = if seen == shown {
                left
            } else {
                (floor + spare * u64::from(*count) / total).min(left)
            };
            left -= width;
            width
        })
        .collect()
}

/// One stacked bar: `segments` are a style class and a count each, in order.
fn bar(out: &mut String, segments: &[(&str, u32)], floor: u64) {
    let counts: Vec<u32> = segments.iter().map(|(_, count)| *count).collect();
    out.push_str(
        "<svg class=\"segbar\" viewBox=\"0 0 1000 10\" preserveAspectRatio=\"none\" \
         aria-hidden=\"true\" focusable=\"false\">",
    );
    let mut x = 0;
    for ((class, _), width) in segments.iter().zip(widths(&counts, floor)) {
        if width > 0 {
            let _ = write!(
                out,
                "<rect class=\"s-{class}\" x=\"{x}\" y=\"0\" width=\"{width}\" height=\"10\"/>"
            );
            x += width;
        }
    }
    out.push_str("</svg>");
}

/// The counts as two bars: the rules the session was checked against, and the
/// rules it was not. The tally under them names every number drawn.
pub(super) fn breakdown(out: &mut String, totals: &Totals) {
    let checked = totals.pass + totals.fail + totals.warn;
    let unchecked =
        totals.not_observed + totals.not_applicable + totals.excluded + totals.unsupported;
    let _ = write!(
        out,
        "<div class=\"breakdown\">\n<div class=\"bk-row\"><p class=\"bk-label\"><strong>{}</strong> \
         checked against this session</p>",
        plural(checked, "rule", "rules")
    );
    bar(
        out,
        &[
            ("fail", totals.fail),
            ("warn", totals.warn),
            ("pass", totals.pass),
        ],
        12,
    );
    let _ = write!(
        out,
        "</div>\n<div class=\"bk-row\"><p class=\"bk-label\"><strong>{}</strong> not checked \
         here; the counts below say why</p>",
        plural(unchecked, "rule", "rules")
    );
    bar(
        out,
        &[
            ("unsupported", totals.unsupported),
            ("not-observed", totals.not_observed),
            ("not-applicable", totals.not_applicable),
            ("excluded", totals.excluded),
        ],
        12,
    );
    out.push_str("</div>\n</div>\n");
}

/// The area of a requirement ID: its prefix (`BASE` of `BASE-003`).
fn area(id: &str) -> &str {
    id.split_once('-').map_or(id, |(area, _)| area)
}

/// An area's name in words, where the page knows one.
fn area_name(area: &str) -> Option<&'static str> {
    Some(match area {
        "AUTH" => "Authorization",
        "BASE" => "Base protocol",
        "CACH" => "Caching",
        "CANC" => "Cancellation",
        "COMP" => "Completion",
        "DISC" => "Discovery",
        "ELIC" => "Elicitation",
        "LIFE" => "Lifecycle",
        "LOG" => "Logging",
        "MRTR" => "Multi round-trip requests",
        "PAGE" => "Pagination",
        "PING" => "Ping",
        "PROG" => "Progress",
        "PROM" => "Prompts",
        "RES" => "Resources",
        "ROOT" => "Roots",
        "SAMP" => "Sampling",
        "SUBS" => "Subscriptions",
        "TASK" => "Tasks",
        "TOOL" => "Tools",
        "TRAN" => "Transports",
        "VERS" => "Versioning",
        _ => return None,
    })
}

/// What an area's bar and row count.
#[derive(Default, Clone, Copy)]
struct AreaCount {
    fail: u32,
    warn: u32,
    pass: u32,
    unsupported: u32,
    unchecked: u32,
}

/// `rows` counted by area, in the order the areas first appear.
fn by_area(rows: &[RequirementReport]) -> Vec<(&str, AreaCount)> {
    let mut areas: Vec<(&str, AreaCount)> = Vec::new();
    for row in rows {
        let name = area(&row.id);
        let index = areas
            .iter()
            .position(|(known, _)| *known == name)
            .unwrap_or_else(|| {
                areas.push((name, AreaCount::default()));
                areas.len() - 1
            });
        let Some((_, count)) = areas.get_mut(index) else {
            continue;
        };
        match row.outcome {
            Outcome::Fail => count.fail += 1,
            Outcome::Warn => count.warn += 1,
            Outcome::Pass => count.pass += 1,
            Outcome::Unsupported => count.unsupported += 1,
            Outcome::Excluded | Outcome::NotApplicable | Outcome::NotObserved => {
                count.unchecked += 1;
            }
        }
    }
    areas
}

/// A numeric cell; a zero is dimmed, so the eye lands on what is there.
fn cell(out: &mut String, class: &str, count: u32) {
    let zero = if count == 0 { " zero" } else { "" };
    let _ = write!(out, "<td class=\"num n-{class}{zero}\">{count}</td>");
}

/// One revision's table: a row per area, its counts and its bar.
fn area_table(out: &mut String, report: &Report) {
    let unsupported = report.totals.unsupported > 0;
    out.push_str(
        "<div class=\"table-scroll\" tabindex=\"0\" role=\"group\" aria-label=\"Results by area, a table\">\
         <table class=\"areas\">\n<thead><tr><th scope=\"col\">Area</th>\
         <th scope=\"col\" class=\"num\">Failed</th><th scope=\"col\" class=\"num\">Warned</th>\
         <th scope=\"col\" class=\"num\">Passed</th>",
    );
    if unsupported {
        out.push_str("<th scope=\"col\" class=\"num\">Unsupported</th>");
    }
    out.push_str(
        "<th scope=\"col\" class=\"num\">Not checked</th>\
         <th scope=\"col\" class=\"bar-col\">Share of the area</th></tr></thead>\n<tbody>\n",
    );
    for (area, count) in by_area(&report.requirements) {
        let quiet = count.fail + count.warn + count.pass + count.unsupported == 0;
        let _ = write!(
            out,
            "<tr{}><th scope=\"row\">",
            if quiet { " class=\"quiet\"" } else { "" }
        );
        if let Some(name) = area_name(area) {
            let _ = write!(out, "<span class=\"area-name\">{name}</span> ");
        }
        let _ = write!(out, "<code class=\"area-id\">{}</code></th>", Text(area));
        cell(out, "fail", count.fail);
        cell(out, "warn", count.warn);
        cell(out, "pass", count.pass);
        if unsupported {
            cell(out, "unsupported", count.unsupported);
        }
        cell(out, "unchecked", count.unchecked);
        out.push_str("<td class=\"bar-col\">");
        bar(
            out,
            &[
                ("fail", count.fail),
                ("warn", count.warn),
                ("unsupported", count.unsupported),
                ("pass", count.pass),
                ("unchecked", count.unchecked),
            ],
            40,
        );
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody>\n</table></div>\n");
}

/// The "results by area" section: where in the protocol the findings are, and
/// how much of each area the session exercised.
pub(super) fn areas(out: &mut String, scope: &Scope<'_>) {
    open_section(out, scope, "areas", "Results by area");
    out.push_str(
        "<p class=\"lede\">The same counts, split by the part of the protocol each rule belongs \
         to. \u{201c}Not checked\u{201d} gathers the rules this session never exercised, those \
         gated on a capability it did not declare, and those no recording can be judged \
         by.</p>\n",
    );
    for report in scope.reports {
        revision_heading(out, scope, report);
        area_table(out, report);
    }
    out.push_str("</section>\n");
}

#[cfg(test)]
mod tests {
    use super::widths;

    #[test]
    fn widths_fill_the_bar_and_keep_small_segments_visible() {
        assert_eq!(widths(&[0, 0], 12), [0, 0]);
        assert_eq!(widths(&[5], 12), [1000]);
        let one_in_many = widths(&[1, 0, 999], 12);
        assert!(one_in_many[0] >= 12, "{one_in_many:?}");
        assert_eq!(one_in_many[1], 0);
        assert_eq!(one_in_many.iter().sum::<u64>(), 1000);
        assert_eq!(widths(&[1, 1], 40), [500, 500]);
    }
}
