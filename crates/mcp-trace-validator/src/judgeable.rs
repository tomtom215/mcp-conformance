// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Whether a run judged anything at all, and what to say when it did not.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use mcp_trace_validator::multi::MultiReport;
use mcp_trace_validator::report::Totals;

#[cfg(test)]
mod tests;

/// Refuses a run in which no requirement could be judged.
///
/// The parser deliberately accepts an empty document — it is well-formed JSON
/// Lines — and the engine then answers honestly: nothing was judged, so there
/// are no findings, so the verdict is `pass` and the exit code is `0`. Every
/// number in that report is true and the conclusion a CI job draws from it is
/// false, because the overwhelmingly likely cause is that the capture failed.
/// This project has been bitten by exactly that: the server tap keyed on a
/// session ID the `2026-07-28` revision had removed and dropped every exchange,
/// leaving "an empty trace directory, indistinguishable from a server nobody
/// talked to".
///
/// The condition is the honest one — zero clauses judged, rather than zero
/// bytes — so a recording carrying only a transport opening and closing is
/// caught too. It cannot fire on a real session: any message at all judges the
/// envelope clauses.
///
/// Two other ways to judge nothing are deliberately *not* this. A registry
/// naming checks the build lacks reports `unsupported` and already exits
/// non-zero with a report that says which; a registry of nothing but exclusions
/// has no judgeable clause for any trace to reach. Both are properties of the
/// registry, and blaming the recording for them would be a wrong diagnosis
/// dressed as a helpful one — so the trace is only accused when there were
/// judgeable clauses and every one of them reported *not observed*.
///
/// `EXIT_USAGE`, because asking for a verdict on a session that was never
/// recorded is a mistake in the asking. The library still answers for anyone
/// who genuinely wants the empty report.
/// Returns the diagnostic to print, or `None` when the run is judgeable.
///
/// Split from the printing so the decision and its wording are both reachable
/// from a unit test: routed only through the binary, every arm of the condition
/// and both halves of the sum survived mutation, because the corpus happens not
/// to contain a trace that separates them.
pub(crate) fn refusal(totals: Totals, trace_source: &str) -> Option<String> {
    let judged = totals.pass + totals.fail + totals.warn;
    if judged > 0 || totals.unsupported > 0 || totals.not_observed == 0 {
        return None;
    }
    let source = if trace_source == "-" {
        "the trace on stdin"
    } else {
        trace_source
    };
    Some(format!(
        "error: {source} judged no requirement at all — an empty or contentless \
         trace is a capture that failed, not a session that conformed"
    ))
}

pub(crate) fn reject(totals: Totals, trace_source: &str) -> bool {
    refusal(totals, trace_source).is_some_and(|message| {
        eprintln!("{message}");
        true
    })
}

/// Every judged revision's totals summed.
///
/// Only for the "judged nothing at all" question, where the sum is the right
/// reading: one revision judging nothing is ordinary — a `2025-11-25` session
/// says little about `2026-07-28`'s clauses — while none of them judging
/// anything is a trace with no content. It is not a report anyone should read,
/// which is why it stays here rather than becoming a method on `MultiReport`.
pub(crate) fn combined(report: &MultiReport) -> Totals {
    report
        .summaries
        .iter()
        .fold(Totals::default(), |mut sum, summary| {
            sum.pass += summary.totals.pass;
            sum.fail += summary.totals.fail;
            sum.warn += summary.totals.warn;
            sum.unsupported += summary.totals.unsupported;
            sum.not_observed += summary.totals.not_observed;
            sum
        })
}
