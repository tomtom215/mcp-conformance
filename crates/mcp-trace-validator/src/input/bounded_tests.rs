// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for reading a trace in memory bounded by its limits.

#![allow(clippy::unwrap_used)]

use std::io::{BufReader, Cursor, Read as _};

use mcp_trace_validator::reader::Limits;

use super::{MEASURE_CEILING, read_bounded};

const EVENT: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#;

#[test]
fn an_endless_line_ends_at_the_ceiling_with_a_lower_bound() {
    let limits = Limits::new(10, 64);
    let endless = BufReader::new(std::io::repeat(b'a'));
    let (bytes, overlong) = read_bounded(endless, &limits).unwrap();
    let overlong = overlong.unwrap();
    assert_eq!(bytes, Vec::<u8>::new());
    assert_eq!((overlong.line, overlong.complete), (1, false));
    assert!(
        overlong.length >= 64 * MEASURE_CEILING,
        "{}",
        overlong.length
    );
}

/// A terminator is not part of the record, as for the parser's `str::lines`,
/// so `\n`, `\r\n` and none all measure the same 200 bytes.
#[test]
fn an_overlong_line_is_measured_exactly_and_the_lines_before_it_are_kept() {
    for ending in ["\n", "\r\n", ""] {
        // Unterminated, the long line is the last one.
        let after = if ending.is_empty() {
            String::new()
        } else {
            format!("{EVENT}\n")
        };
        let document = format!("{EVENT}\n{}{ending}{after}", "b".repeat(200));
        let limits = Limits::new(10, 150);
        let (bytes, overlong) = read_bounded(Cursor::new(document), &limits).unwrap();
        let overlong = overlong.unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), format!("{EVENT}\n"));
        assert_eq!(
            (overlong.line, overlong.length, overlong.complete),
            (2, 200, true),
            "{ending:?}"
        );
    }
}

#[test]
fn reading_stops_after_the_line_the_event_limit_refuses() {
    let document = format!("{EVENT}\n").repeat(1_000);
    let (bytes, overlong) = read_bounded(Cursor::new(document), &Limits::new(3, 4096)).unwrap();
    assert!(overlong.is_none());
    assert_eq!(String::from_utf8(bytes).unwrap().lines().count(), 4);
}

#[test]
fn a_document_within_its_limits_is_read_whole() {
    let document = format!("{EVENT}\n{EVENT}");
    let mut copy = Vec::new();
    Cursor::new(document.clone())
        .read_to_end(&mut copy)
        .unwrap();
    let (bytes, overlong) = read_bounded(Cursor::new(document), &Limits::default()).unwrap();
    assert!(overlong.is_none());
    assert_eq!(bytes, copy);
}
