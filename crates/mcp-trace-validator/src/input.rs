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
    let bytes = read_document(source).map_err(|message| {
        eprintln!("error: {message}");
        EXIT_USAGE
    })?;
    // JSON Lines is UTF-8; bytes that are not are a malformed trace (exit 3),
    // located like every other malformation, not an unreadable file (exit 2).
    let document = String::from_utf8(bytes).map_err(|error| {
        let valid = &error.as_bytes()[..error.utf8_error().valid_up_to()];
        let line = valid.split(|byte| *byte == b'\n').count();
        eprintln!(
            "error: malformed trace: line {line}: not valid UTF-8 (JSON Lines must be UTF-8)"
        );
        if error.as_bytes().starts_with(&[0xFF, 0xFE])
            || error.as_bytes().starts_with(&[0xFE, 0xFF])
        {
            eprintln!(
                "hint: the file is UTF-16, as Windows PowerShell's `>` writes; re-encode it, \
                 e.g. `Get-Content in.jsonl | Set-Content -Encoding utf8 out.jsonl`"
            );
        }
        EXIT_MALFORMED_TRACE
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

fn read_document(source: &str) -> Result<Vec<u8>, String> {
    if source == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read stdin: {error}"))?;
        Ok(bytes)
    } else {
        fs::read(source).map_err(|error| format!("cannot read {source}: {error}"))
    }
}
