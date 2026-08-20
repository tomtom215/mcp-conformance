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

/// Why a trace document was rejected. Every variant that addresses one record
/// carries its 1-based line number; the two that describe the document as a
/// whole do not.
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
    /// The document begins with a UTF-8 byte-order mark.
    ///
    /// Its own variant because serde reports it as `expected value at line 1
    /// column 1`, which is true and tells the reader nothing: the offending
    /// bytes are invisible in every editor that wrote them. A BOM is the
    /// commonest way a trace produced on Windows fails to parse — `Out-File`
    /// and `Set-Content` have both emitted one by default — and
    /// `jq`, `python -m json.tool` and every other tool the reader might reach
    /// for will insist the file is fine.
    ByteOrderMark,
    /// The whole document is a single JSON value: it is JSON, not JSON Lines.
    ///
    /// Checked only after a line has already failed, so a valid document never
    /// reaches it and the cost is paid once, on the error path. Pretty-printing
    /// is the other half of the same mistake as the BOM — a file that is
    /// obviously well-formed JSON, rejected with a message about column 1.
    NotJsonLines {
        /// Whether that value is an array, so the message can name the fix.
        array: bool,
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
            Self::ByteOrderMark => write!(
                f,
                "the document begins with a UTF-8 byte-order mark (EF BB BF), which JSON \
                 Lines does not permit; strip those three bytes and re-run"
            ),
            Self::NotJsonLines { array } => {
                let fix = if *array {
                    "it is a JSON array — one event per element, so `jq -c '.[]' <file>` converts it"
                } else {
                    "it is one pretty-printed JSON object — `jq -c . <file>` puts it on one line"
                };
                write!(
                    f,
                    "the document is a single JSON value, not JSON Lines (one event per line): {fix}"
                )
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
    // Before anything is addressed by line: a leading BOM makes line 1 fail with
    // a message about a column, and the bytes it names cannot be seen.
    if input.starts_with('\u{feff}') {
        return Err(TraceParseError::ByteOrderMark);
    }
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
            // A pretty-printed document with an internal blank line reaches
            // here before any line has failed to parse, so ask the same
            // question the parse-failure path asks.
            return Err(whole_document_is_json(input)
                .unwrap_or(TraceParseError::BlankLine { line: line_number }));
        }
        if events.len() >= limits.max_events {
            return Err(TraceParseError::TooManyEvents {
                limit: limits.max_events,
            });
        }
        let event: TraceEvent = serde_json::from_str(line).map_err(|source| {
            whole_document_is_json(input).unwrap_or(TraceParseError::Malformed {
                line: line_number,
                source,
            })
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

/// [`TraceParseError::NotJsonLines`] when `input` is a JSON *document* rather
/// than JSON Lines.
///
/// Parsing end to end is not enough on its own, and the test that says so was
/// already here: one valid record followed by a stray blank line also parses as
/// a single value, because JSON permits trailing whitespace — reporting that as
/// "this is JSON, not JSON Lines" would replace a true diagnosis with a false
/// one. So the value must also be shaped like a document a person pretty-printed
/// or wrapped: an array (the whole trace as one value, however it is spaced), or
/// a value spread across more than one line. A one-line object that simply is
/// not a trace event falls through to serde's message, which names the field it
/// was missing.
fn whole_document_is_json(input: &str) -> Option<TraceParseError> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let array = value.is_array();
    let spans_lines = input.lines().filter(|line| !line.trim().is_empty()).count() > 1;
    (array || spans_lines).then_some(TraceParseError::NotJsonLines { array })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const VALID_EVENT: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#;

    /// The two ways a first trace fails to parse for a reason serde cannot
    /// name, and the message each must produce.
    #[test]
    fn a_leading_byte_order_mark_is_named_rather_than_pointed_at() {
        let document = format!("\u{feff}{VALID_EVENT}");
        let error = parse_trace(&document, &Limits::default()).unwrap_err();
        assert!(matches!(error, TraceParseError::ByteOrderMark), "{error:?}");
        let message = error.to_string();
        assert!(message.contains("byte-order mark"), "{message}");
        assert!(message.contains("EF BB BF"), "{message}");
        // The same bytes anywhere but the start are ordinary content, and the
        // line that carries them is what the reader is told about.
        let inside = format!("{VALID_EVENT}\n\u{feff}{VALID_EVENT}");
        assert!(matches!(
            parse_trace(&inside, &Limits::default()).unwrap_err(),
            TraceParseError::Malformed { line: 2, .. }
        ));
    }

    #[test]
    fn a_json_document_is_told_apart_from_json_lines() {
        // A pretty-printed object: serde says `EOF while parsing an object at
        // line 1 column 1`, which describes a fragment rather than the file.
        let pretty = "{\n  \"seq\": 0,\n  \"direction\": \"client-to-server\",\n  \"transport\": \"stdio\",\n  \"kind\": \"lifecycle\",\n  \"event\": \"transport-open\"\n}";
        let error = parse_trace(pretty, &Limits::default()).unwrap_err();
        assert!(
            matches!(error, TraceParseError::NotJsonLines { array: false }),
            "{error:?}"
        );
        assert!(error.to_string().contains("jq -c ."), "{error}");

        // The same events as a JSON array, pretty or compact.
        for document in [format!("[{VALID_EVENT}]"), format!("[\n  {VALID_EVENT}\n]")] {
            let error = parse_trace(&document, &Limits::default()).unwrap_err();
            assert!(
                matches!(error, TraceParseError::NotJsonLines { array: true }),
                "{error:?}"
            );
            assert!(error.to_string().contains("jq -c '.[]'"), "{error}");
        }
    }

    #[test]
    fn one_record_and_a_stray_newline_is_still_a_blank_line() {
        // JSON permits trailing whitespace, so this parses as a single value —
        // and calling it "a JSON document, not JSON Lines" would be a confident
        // wrong answer where the true one is one word away.
        let document = format!("{VALID_EVENT}\n\n");
        assert!(matches!(
            parse_trace(&document, &Limits::default()).unwrap_err(),
            TraceParseError::BlankLine { line: 2 }
        ));
    }

    #[test]
    fn a_one_line_object_that_is_not_an_event_keeps_serdes_message() {
        // Also one JSON value, also not JSON Lines by shape — but the useful
        // answer names the field, not the file format.
        let error = parse_trace(r#"{"hello":"world"}"#, &Limits::default()).unwrap_err();
        assert!(
            matches!(error, TraceParseError::Malformed { line: 1, .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("missing field `seq`"), "{error}");
    }

    #[test]
    fn a_genuinely_broken_line_still_gets_its_line_number() {
        // The check must not swallow the ordinary case: a document that is not
        // one JSON value keeps serde's message and the line it happened on.
        let document = format!("{VALID_EVENT}\nnot json\n");
        let error = parse_trace(&document, &Limits::default()).unwrap_err();
        assert!(
            matches!(error, TraceParseError::Malformed { line: 2, .. }),
            "{error:?}"
        );

        // And a stray blank line between real records is still a blank line.
        let gapped = format!("{VALID_EVENT}\n\n{VALID_EVENT}\n");
        assert!(matches!(
            parse_trace(&gapped, &Limits::default()).unwrap_err(),
            TraceParseError::BlankLine { line: 2 }
        ));
    }

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
