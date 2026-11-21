// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the shared report arguments tell the command that holds them
//! (`mcp-conformance check` reads them through these accessors).

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use clap::Parser;

use super::{ReportArgs, ValidateArgs};

/// `validate`'s arguments, parsed as the binaries parse them.
#[derive(Debug, Parser)]
struct Harness {
    #[command(flatten)]
    args: ValidateArgs,
}

fn report(flags: &[&str]) -> ReportArgs {
    let line = ["validate", "trace.jsonl"].iter().chain(flags);
    Harness::try_parse_from(line).unwrap().args.report
}

#[test]
fn the_html_report_is_where_the_flag_says_and_nowhere_by_default() {
    assert_eq!(report(&[]).html_report(), None);
    let named = report(&["--html-report", "out/page.html"]);
    assert_eq!(named.html_report(), Some(Path::new("out/page.html")));
}

#[test]
fn a_default_page_fills_in_only_when_the_flag_named_none() {
    let filled = report(&[]).with_html_report(PathBuf::from("report.html"));
    assert_eq!(filled.html_report(), Some(Path::new("report.html")));
    let named = report(&["--html-report", "mine.html"]).with_html_report("report.html".into());
    assert_eq!(named.html_report(), Some(Path::new("mine.html")));
}

#[test]
fn only_the_human_format_is_human() {
    assert!(report(&[]).is_human(), "the default format");
    assert!(report(&["--format", "human"]).is_human());
    for format in ["json", "junit", "sarif", "html"] {
        assert!(!report(&["--format", format]).is_human(), "{format}");
    }
}
