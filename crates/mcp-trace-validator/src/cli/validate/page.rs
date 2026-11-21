// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `--html-report`: the HTML report written to a file beside whatever
//! `--format` prints, so one run gives the terminal its text (or CI its
//! `JUnit`) and a person the page.

use std::path::Path;

use super::{Judged, baseline, page_trace};
use crate::html;
use crate::report::{Report, Unjudged};

/// Writes the run's HTML report to `path`, returning whether it was written.
/// The page is the one `--format html` prints for the same run.
pub(super) fn write(
    path: &Path,
    judged: &[Judged],
    unjudged: &[Unjudged],
    run: &baseline::Run,
) -> bool {
    let traces: Vec<(html::Trace<'_>, &[Report])> = judged
        .iter()
        .map(|one| (page_trace(&one.trace, &one.events), one.reports.as_slice()))
        .collect();
    let page = html::render_run_with_baseline(&traces, unjudged, &run.notes);
    match std::fs::write(path, page) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "error: cannot write the HTML report to {}: {error}",
                path.display()
            );
            false
        }
    }
}
