// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reading the trace a `validate` run names: a file or stdin, parsed under the
//! limits the command line set.

// Same trade as `judgeable`: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about items in a binary's private module.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::io::{BufRead as _, Read as _};
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
    let (bytes, overlong) = read_document(source, limits)?;
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
    let events = reader::parse_trace(&document, limits).map_err(|error| Refusal {
        error: format!("{prefix}malformed trace: {error}"),
        hints: parse_hint(&error, &document).into_iter().collect(),
        code: EXIT_MALFORMED_TRACE,
    })?;
    // Every line before the over-long one parsed, so it is the first problem.
    match overlong {
        None => Ok(events),
        Some(Overlong {
            line,
            length,
            complete,
        }) => {
            let limit = limits.max_line_bytes;
            let (size, at_least) = if complete {
                ("", "")
            } else {
                ("over ", " or more")
            };
            Err(Refusal {
                error: format!(
                    "{prefix}malformed trace: line {line}: record is {size}{length} bytes, \
                     exceeding the {limit}-byte limit"
                ),
                hints: vec![format!(
                    "if the recording is sound, re-run with --max-line-bytes {length}{at_least}"
                )],
                code: EXIT_MALFORMED_TRACE,
            })
        }
    }
}

/// A line longer than `--max-line-bytes`, found while reading.
struct Overlong {
    /// 1-based line number.
    line: usize,
    /// Its length in bytes, or a lower bound when not `complete`.
    length: usize,
    /// Whether the line ended before [`MEASURE_CEILING`] times the limit.
    complete: bool,
}

/// How far past the limit an over-long line is measured, as a multiple of the
/// limit: far enough to give the exact length any real recording needs, near
/// enough that endless input (`/dev/zero`) ends in bounded time.
const MEASURE_CEILING: usize = 16;

/// Reads a trace no further than its limits can use, in memory bounded by them.
///
/// Reading the whole document first let a 1 GiB file occupy 1 GiB before the
/// line limit refused its first line, and `/dev/zero` grow without end. Lines
/// are read one at a time: the read stops after the line the event limit
/// refuses, and at a line over the length limit, which is measured — its
/// bytes counted, not kept — and returned beside the lines before it, so that
/// an earlier line's error still comes first.
fn read_bounded(
    mut input: impl std::io::BufRead,
    limits: &Limits,
) -> std::io::Result<(Vec<u8>, Option<Overlong>)> {
    let limit = limits.max_line_bytes;
    // The content limit plus a `\r\n` terminator.
    let per_line = u64::try_from(limit.saturating_add(2)).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    let mut lines = 0usize;
    loop {
        let start = bytes.len();
        let read = std::io::Read::by_ref(&mut input)
            .take(per_line)
            .read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok((bytes, None));
        }
        lines += 1;
        if bytes.last() != Some(&b'\n') {
            if read < limit.saturating_add(2) {
                return Ok((bytes, None)); // The last line, unterminated.
            }
            bytes.truncate(start);
            let (length, complete) = measure(&mut input, read, limit)?;
            let overlong = Overlong {
                line: lines,
                length,
                complete,
            };
            return Ok((bytes, Some(overlong)));
        }
        if lines > limits.max_events {
            return Ok((bytes, None)); // The parser refuses this line, or one before it.
        }
    }
}

/// The length of a line already `read` bytes long, counted to its end or to
/// [`MEASURE_CEILING`] times `limit`, without keeping it; and whether it ended.
fn measure(
    input: &mut impl std::io::BufRead,
    read: usize,
    limit: usize,
) -> std::io::Result<(usize, bool)> {
    let ceiling = limit.saturating_mul(MEASURE_CEILING);
    let mut length = read;
    let mut carriage_return = false;
    while length < ceiling {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok((length, true));
        }
        if let Some(end) = chunk.iter().position(|byte| *byte == b'\n') {
            let before_newline = end
                .checked_sub(1)
                .map(|at| chunk[at])
                .or_else(|| carriage_return.then_some(b'\r'));
            length += end;
            input.consume(end + 1);
            // `str::lines`, which the parser uses, does not count a `\r` ending the line.
            if before_newline == Some(b'\r') {
                length -= 1;
            }
            return Ok((length, true));
        }
        carriage_return = chunk.last() == Some(&b'\r');
        length += chunk.len();
        let consumed = chunk.len();
        input.consume(consumed);
    }
    Ok((length, false))
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

fn read_document(source: &str, limits: &Limits) -> Result<(Vec<u8>, Option<Overlong>), Refusal> {
    if source == "-" {
        return read_bounded(std::io::stdin().lock(), limits)
            .map_err(|error| unreadable(format!("cannot read stdin: {error}"), None));
    }
    fs::File::open(source)
        .and_then(|file| read_bounded(std::io::BufReader::new(file), limits))
        .map_err(|error| {
            let path = Path::new(source);
            // The two ways a shell hands over a name that is not a trace file.
            let hint = if path.is_dir() {
                Some(format!(
                    "{source} is a directory; pass the traces in it, e.g. `{}`",
                    path.join("*.jsonl").display()
                ))
            } else if error.kind() == std::io::ErrorKind::NotFound
                && source.contains(['*', '?', '['])
            {
                Some(
                    "no file matched this pattern, so the shell passed it on unexpanded".to_owned(),
                )
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

#[cfg(test)]
mod bounded_tests;
