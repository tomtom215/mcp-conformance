// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Trace parsing with hard resource limits.
//!
//! Traces are untrusted input by design — they arrive from arbitrary implementations
//! and arbitrary capture tooling. The reader therefore enforces explicit caps (line
//! length, event count) and produces typed, line-addressed errors instead of panics.
//! It performs no I/O itself: callers hand it the document text.

use core::fmt;

use mcp_conformance_core::trace::TraceEvent;

/// Resource limits applied while parsing a trace document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Maximum number of events accepted in one trace.
    pub max_events: usize,
    /// Maximum length in bytes of a single JSON Lines record.
    pub max_line_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // Generous for real sessions (the everything-server suites produce
            // hundreds of events) while bounding adversarial inputs.
            max_events: 100_000,
            max_line_bytes: 1024 * 1024,
        }
    }
}

/// Why a trace document was rejected. Every variant carries the 1-based line number.
#[derive(Debug)]
#[non_exhaustive]
pub enum TraceParseError {
    /// A line exceeded [`Limits::max_line_bytes`].
    LineTooLong {
        /// 1-based line number.
        line: usize,
        /// Observed length in bytes.
        length: usize,
        /// The configured cap.
        limit: usize,
    },
    /// The document contains more than [`Limits::max_events`] events.
    TooManyEvents {
        /// The configured cap.
        limit: usize,
    },
    /// A line was empty (JSON Lines forbids blank records; a single trailing newline
    /// is fine).
    BlankLine {
        /// 1-based line number.
        line: usize,
    },
    /// A line was not a valid [`TraceEvent`] object.
    Malformed {
        /// 1-based line number.
        line: usize,
        /// The underlying JSON error.
        source: serde_json::Error,
    },
    /// Event `seq` values must be strictly increasing in document order.
    NonMonotonicSeq {
        /// 1-based line number.
        line: usize,
        /// The `seq` on this line.
        seq: u64,
        /// The `seq` on the previous event line.
        previous: u64,
    },
}

impl fmt::Display for TraceParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineTooLong {
                line,
                length,
                limit,
            } => write!(
                f,
                "line {line}: record is {length} bytes, exceeding the {limit}-byte limit"
            ),
            Self::TooManyEvents { limit } => {
                write!(f, "trace exceeds the {limit}-event limit")
            }
            Self::BlankLine { line } => {
                write!(
                    f,
                    "line {line}: blank line (JSON Lines forbids blank records)"
                )
            }
            Self::Malformed { line, source } => {
                write!(f, "line {line}: not a valid trace event: {source}")
            }
            Self::NonMonotonicSeq {
                line,
                seq,
                previous,
            } => write!(
                f,
                "line {line}: seq {seq} is not greater than the previous event's seq {previous}"
            ),
        }
    }
}

impl core::error::Error for TraceParseError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Malformed { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Parses a JSON Lines trace document into events, enforcing [`Limits`].
///
/// # Errors
///
/// Returns the first [`TraceParseError`] encountered, addressed by 1-based line
/// number. An empty document yields an empty event list (validating an empty trace is
/// the engine's question, not the parser's).
///
/// ```
/// use mcp_trace_validator::reader::{Limits, parse_trace};
///
/// let line = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#;
/// assert_eq!(parse_trace(line, &Limits::default())?.len(), 1);
/// assert!(parse_trace("not json", &Limits::default()).is_err());
/// # Ok::<(), mcp_trace_validator::reader::TraceParseError>(())
/// ```
pub fn parse_trace(input: &str, limits: &Limits) -> Result<Vec<TraceEvent>, TraceParseError> {
    let mut events = Vec::new();
    let mut previous_seq: Option<u64> = None;
    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        if line.len() > limits.max_line_bytes {
            return Err(TraceParseError::LineTooLong {
                line: line_number,
                length: line.len(),
                limit: limits.max_line_bytes,
            });
        }
        if line.trim().is_empty() {
            return Err(TraceParseError::BlankLine { line: line_number });
        }
        if events.len() >= limits.max_events {
            return Err(TraceParseError::TooManyEvents {
                limit: limits.max_events,
            });
        }
        let event: TraceEvent =
            serde_json::from_str(line).map_err(|source| TraceParseError::Malformed {
                line: line_number,
                source,
            })?;
        if let Some(previous) = previous_seq
            && event.seq <= previous
        {
            return Err(TraceParseError::NonMonotonicSeq {
                line: line_number,
                seq: event.seq,
                previous,
            });
        }
        previous_seq = Some(event.seq);
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const VALID_EVENT: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#;

    #[test]
    fn parses_valid_lines_and_empty_documents() {
        assert!(parse_trace("", &Limits::default()).unwrap().is_empty());
        let one = parse_trace(VALID_EVENT, &Limits::default()).unwrap();
        assert_eq!(one.len(), 1);
        // Trailing newline is fine.
        let with_newline = format!("{VALID_EVENT}\n");
        assert_eq!(
            parse_trace(&with_newline, &Limits::default())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn rejects_blank_interior_lines() {
        let doc = format!("{VALID_EVENT}\n\n");
        assert!(matches!(
            parse_trace(&doc, &Limits::default()),
            Err(TraceParseError::BlankLine { line: 2 })
        ));
    }

    #[test]
    fn rejects_oversized_lines() {
        let limits = Limits {
            max_line_bytes: 16,
            ..Limits::default()
        };
        assert!(matches!(
            parse_trace(VALID_EVENT, &limits),
            Err(TraceParseError::LineTooLong { line: 1, .. })
        ));
    }

    #[test]
    fn rejects_too_many_events() {
        let limits = Limits {
            max_events: 1,
            ..Limits::default()
        };
        let second = VALID_EVENT.replace("\"seq\":0", "\"seq\":1");
        let doc = format!("{VALID_EVENT}\n{second}");
        assert!(matches!(
            parse_trace(&doc, &limits),
            Err(TraceParseError::TooManyEvents { limit: 1 })
        ));
    }

    #[test]
    fn rejects_malformed_records_with_line_numbers() {
        let doc = format!("{VALID_EVENT}\n{{\"seq\":1}}");
        match parse_trace(&doc, &Limits::default()) {
            Err(TraceParseError::Malformed { line, .. }) => assert_eq!(line, 2),
            other => panic!("expected malformed at line 2, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_monotonic_seq() {
        let duplicate = format!("{VALID_EVENT}\n{VALID_EVENT}");
        assert!(matches!(
            parse_trace(&duplicate, &Limits::default()),
            Err(TraceParseError::NonMonotonicSeq {
                line: 2,
                seq: 0,
                previous: 0
            })
        ));
    }

    #[test]
    fn error_messages_are_line_addressed() {
        let doc = format!("{VALID_EVENT}\nnot json");
        let error = parse_trace(&doc, &Limits::default()).unwrap_err();
        assert!(error.to_string().starts_with("line 2:"), "{error}");
    }

    #[test]
    fn line_exactly_at_the_byte_limit_is_accepted() {
        // Boundary pinning: the limit is inclusive (> rejects, == passes).
        let limits = Limits {
            max_line_bytes: VALID_EVENT.len(),
            ..Limits::default()
        };
        assert_eq!(parse_trace(VALID_EVENT, &limits).unwrap().len(), 1);
    }

    #[test]
    fn error_source_is_exposed_for_malformed_records_only() {
        use core::error::Error as _;
        let malformed = parse_trace("nope", &Limits::default()).unwrap_err();
        assert!(malformed.source().is_some());
        let blank = parse_trace(" \n", &Limits::default()).unwrap_err();
        assert!(blank.source().is_none());
    }
}
