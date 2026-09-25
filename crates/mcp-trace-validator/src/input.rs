// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reading the trace a `validate` run names: a file or stdin, parsed under the
//! limits the command line set.

// Same trade as `judgeable`: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about items in a binary's private module.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::io::Read as _;

use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::reader::{self, Limits, TraceParseError};

use crate::{EXIT_MALFORMED_TRACE, EXIT_USAGE};

/// Reads and parses the trace, mapping failures to their exit codes.
pub(crate) fn read_events(source: &str, limits: &Limits) -> Result<Vec<TraceEvent>, u8> {
    let document = read_document(source).map_err(|message| {
        eprintln!("error: {message}");
        EXIT_USAGE
    })?;
    reader::parse_trace(&document, limits).map_err(|error| {
        eprintln!("error: malformed trace: {error}");
        if let Some(hint) = limit_hint(&error) {
            eprintln!("hint: {hint}");
        }
        EXIT_MALFORMED_TRACE
    })
}

/// The flag that lifts the limit a trace ran into. A trace over a limit is
/// usually a long or large session, not a broken one, and the error alone does
/// not say that the limit is the reader's to change.
fn limit_hint(error: &TraceParseError) -> Option<String> {
    match error {
        TraceParseError::LineTooLong { length, .. } => Some(format!(
            "if the recording is sound, re-run with --max-line-bytes {length} or more"
        )),
        TraceParseError::TooManyEvents { limit } => Some(format!(
            "if the recording is sound, re-run with --max-events above {limit}"
        )),
        _ => None,
    }
}

fn read_document(source: &str) -> Result<String, String> {
    if source == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| format!("cannot read stdin: {error}"))?;
        Ok(text)
    } else {
        fs::read_to_string(source).map_err(|error| format!("cannot read {source}: {error}"))
    }
}
