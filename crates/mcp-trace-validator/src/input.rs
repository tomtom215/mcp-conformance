// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reading the trace a `validate` run names: a file or stdin, parsed under the
//! limits the command line set.

// Same trade as `judgeable`: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about items in a binary's private module.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::io::Read as _;
use std::path::Path;

use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::reader::{self, Limits, TraceParseError};

use crate::{EXIT_MALFORMED_TRACE, EXIT_USAGE};

/// Why a trace was not judged: the error line, any hints that follow it, and
/// the exit status it carries.
///
/// Returned rather than printed so a multi-trace run can also put the error in
/// its report — a reader of the JSON or `JUnit` file has no stderr to look at.
#[derive(Debug)]
pub(crate) struct Refusal {
    /// The error, without the `error: ` the terminal line starts with.
    pub(crate) error: String,
    /// What to do about it, one `hint: ` line each.
    pub(crate) hints: Vec<String>,
    pub(crate) code: u8,
}

impl Refusal {
    /// A refusal from a diagnostic already worded as a terminal line
    /// (`error: …`), as `judgeable` words them.
    pub(crate) fn from_diagnostic(diagnostic: &str, code: u8) -> Self {
        Self {
            error: diagnostic
                .strip_prefix("error: ")
                .unwrap_or(diagnostic)
                .to_owned(),
            hints: Vec::new(),
            code,
        }
    }

    /// Prints the error and its hints on stderr.
    pub(crate) fn print(&self) {
        eprintln!("error: {}", self.error);
        for hint in &self.hints {
            eprintln!("hint: {hint}");
        }
    }
}

/// Reads and parses the trace. `prefix` names the trace in every error about
/// its content (`a.jsonl: `) when several are judged in one run, and is empty
/// otherwise; an unreadable file is named either way.
pub(crate) fn read_events(
    source: &str,
    limits: &Limits,
    prefix: &str,
) -> Result<Vec<TraceEvent>, Refusal> {
    let bytes = read_document(source)?;
    // JSON Lines is UTF-8; bytes that are not are a malformed trace (exit 3),
    // located like every other malformation, not an unreadable file (exit 2).
    let document = String::from_utf8(bytes).map_err(|error| {
        let valid = &error.as_bytes()[..error.utf8_error().valid_up_to()];
        let line = valid.split(|byte| *byte == b'\n').count();
        let utf16 = error.as_bytes().starts_with(&[0xFF, 0xFE])
            || error.as_bytes().starts_with(&[0xFE, 0xFF]);
        Refusal {
            error: format!(
                "{prefix}malformed trace: line {line}: not valid UTF-8 (JSON Lines must be UTF-8)"
            ),
            hints: utf16
                .then(|| {
                    "the file is UTF-16, as Windows PowerShell's `>` writes; re-encode it, \
                     e.g. `Get-Content in.jsonl | Set-Content -Encoding utf8 out.jsonl`"
                        .to_owned()
                })
                .into_iter()
                .collect(),
            code: EXIT_MALFORMED_TRACE,
        }
    })?;
    reader::parse_trace(&document, limits).map_err(|error| Refusal {
        error: format!("{prefix}malformed trace: {error}"),
        hints: parse_hint(&error, &document).into_iter().collect(),
        code: EXIT_MALFORMED_TRACE,
    })
}

/// What the error alone does not say: that a limit is the reader's to lift
/// (a trace over one is usually a long session, not a broken one), that the
/// blank line is only the one a stray newline leaves at the end, or that the
/// line is a JSON-RPC message with no event around it.
fn parse_hint(error: &TraceParseError, document: &str) -> Option<String> {
    match error {
        TraceParseError::LineTooLong { length, .. } => Some(format!(
            "if the recording is sound, re-run with --max-line-bytes {length} or more"
        )),
        TraceParseError::TooManyEvents { limit } => Some(format!(
            "if the recording is sound, re-run with --max-events above {limit}"
        )),
        TraceParseError::BlankLine { line } if is_trailing(document, *line) => Some(
            "it is a trailing blank line at the end of the file; delete it — one final \
             newline is fine, an empty line after it is not"
                .to_owned(),
        ),
        TraceParseError::Malformed { line, .. } if is_bare_message(document, *line) => Some(
            "this is a bare JSON-RPC message, not a trace event: a trace wraps each message \
             in an event (`seq`, `direction`, `transport`, `kind`). Record the session with \
             mcp-trace-capture, which writes that form"
                .to_owned(),
        ),
        _ => None,
    }
}

/// Whether every line after 1-based `line`, which is blank, is blank too.
fn is_trailing(document: &str, line: usize) -> bool {
    document
        .lines()
        .skip(line)
        .all(|rest| rest.trim().is_empty())
}

/// Whether 1-based `line` is a JSON-RPC message rather than a trace event: an
/// object carrying `jsonrpc` and no event `kind`.
fn is_bare_message(document: &str, line: usize) -> bool {
    document
        .lines()
        .nth(line - 1)
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .is_some_and(|value| value.get("jsonrpc").is_some() && value.get("kind").is_none())
}

fn read_document(source: &str) -> Result<Vec<u8>, Refusal> {
    if source == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| unreadable(format!("cannot read stdin: {error}"), None))?;
        return Ok(bytes);
    }
    fs::read(source).map_err(|error| {
        let path = Path::new(source);
        // The two ways a shell hands over a name that is not a trace file.
        let hint = if path.is_dir() {
            Some(format!(
                "{source} is a directory; pass the traces in it, e.g. `{}`",
                path.join("*.jsonl").display()
            ))
        } else if error.kind() == std::io::ErrorKind::NotFound && source.contains(['*', '?', '[']) {
            Some("no file matched this pattern, so the shell passed it on unexpanded".to_owned())
        } else {
            None
        };
        unreadable(format!("cannot read {source}: {error}"), hint)
    })
}

fn unreadable(error: String, hint: Option<String>) -> Refusal {
    Refusal {
        error,
        hints: hint.into_iter().collect(),
        code: EXIT_USAGE,
    }
}

#[cfg(test)]
mod tests;
