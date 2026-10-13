// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Judges a recorded trace with the library, as a gate in your own tooling
//! would: print the findings, and exit non-zero unless the session passed.
//!
//! ```text
//! cargo run -p mcp-trace-validator --example judge_file -- corpus/violations/base-003-request-id-reuse.jsonl
//! ```

use std::process::ExitCode;

use mcp_trace_validator::judge;
use mcp_trace_validator::report::Verdict;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: judge_file <trace.jsonl>");
        return ExitCode::from(2);
    };
    let trace = match std::fs::read_to_string(&path) {
        Ok(trace) => trace,
        Err(error) => {
            eprintln!("cannot read {}: {error}", path.to_string_lossy());
            return ExitCode::from(2);
        }
    };
    // Judged against the revision the trace declares, as the CLI does by
    // default; `judge::judge_with` takes `Options` to choose revisions or limits.
    let judgment = match judge::judge(&trace) {
        Ok(judgment) => judgment,
        Err(error) => {
            eprintln!("cannot judge {}: {error}", path.to_string_lossy());
            return ExitCode::from(2);
        }
    };
    for report in &judgment.reports {
        println!("{}", report.render_findings());
    }
    match judgment.verdict() {
        Verdict::Pass | Verdict::PassWithWarnings => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
