// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;

/// Feeds `input` split at `cut` and returns everything produced.
fn lines_split_at(input: &[u8], cut: usize) -> Vec<Line> {
    let mut splitter = LineSplitter::new(1024);
    let mut out = splitter.push(&input[..cut]);
    out.extend(splitter.push(&input[cut..]));
    out.extend(splitter.finish());
    out
}

fn events_split_at(input: &[u8], cut: usize) -> Vec<Vec<u8>> {
    let mut parser = SseParser::new(1024);
    let mut out = parser.push(&input[..cut]);
    out.extend(parser.push(&input[cut..]));
    out
}

#[test]
fn lines_are_the_same_wherever_the_input_is_cut() {
    let input = "{\"a\":\"é\"}\r\n{\"b\":\"日本\"}\n\n{\"c\":1}".as_bytes();
    let whole = lines_split_at(input, 0);
    assert_eq!(
        whole,
        [
            Line::Complete(b"{\"a\":\"\xc3\xa9\"}".to_vec()),
            Line::Complete("{\"b\":\"日本\"}".as_bytes().to_vec()),
            Line::Complete(Vec::new()),
            Line::Complete(b"{\"c\":1}".to_vec()),
        ]
    );
    for cut in 0..=input.len() {
        assert_eq!(lines_split_at(input, cut), whole, "cut at {cut}");
    }
}

#[test]
fn an_oversized_line_is_reported_once_and_does_not_poison_the_next() {
    let mut splitter = LineSplitter::new(8);
    let mut out = splitter.push(b"0123456789");
    out.extend(splitter.push(b"abc\n{\"ok\":1}\n"));
    assert_eq!(
        out,
        [Line::Oversized, Line::Complete(b"{\"ok\":1}".to_vec())]
    );
    // Oversized within a single chunk too.
    let mut splitter = LineSplitter::new(4);
    assert_eq!(
        splitter.push(b"too long\nok\n"),
        [Line::Oversized, Line::Complete(b"ok".to_vec())]
    );
    // And at end of stream.
    let mut splitter = LineSplitter::new(4);
    assert!(splitter.push(b"too long").is_empty());
    assert_eq!(splitter.finish(), Some(Line::Oversized));
}

#[test]
fn blank_and_non_json_lines_are_not_json() {
    assert_eq!(parse_json(b"  "), None);
    assert_eq!(parse_json(b"hello"), None);
    assert_eq!(parse_json(b"{\"x\":1}").unwrap()["x"], 1);
}

#[test]
fn sse_events_are_the_same_wherever_the_stream_is_cut() {
    let input = "\u{FEFF}: comment\r\nevent: message\r\nid: 7\r\ndata: {\"a\":\"é\"}\r\n\r\ndata:{\"b\":\r\ndata: 2}\n\ndata: {\"c\":\"日本\"}\r\r"
        .as_bytes();
    let whole = events_split_at(input, 0);
    assert_eq!(
        whole,
        [
            "{\"a\":\"é\"}".as_bytes().to_vec(),
            b"{\"b\":\n2}".to_vec(),
            "{\"c\":\"日本\"}".as_bytes().to_vec(),
        ]
    );
    for cut in 0..=input.len() {
        assert_eq!(events_split_at(input, cut), whole, "cut at {cut}");
    }
    // Multi-line data is valid JSON once joined.
    assert_eq!(parse_json(&whole[1]).unwrap()["b"], 2);
}

#[test]
fn a_crlf_split_across_chunks_is_one_line_ending() {
    let mut parser = SseParser::new(1024);
    let mut out = parser.push(b"data: 1\r");
    out.extend(parser.push(b"\n\r"));
    out.extend(parser.push(b"\n"));
    assert_eq!(out, [b"1".to_vec()]);
}

#[test]
fn events_without_data_and_empty_data_are_not_dispatched() {
    let mut parser = SseParser::new(1024);
    assert!(parser.push(b"event: ping\n\nid: 3\n\ndata:\n\n").is_empty());
}

#[test]
fn an_oversized_event_is_dropped_and_counted_without_affecting_the_next() {
    let mut parser = SseParser::new(8);
    let out = parser.push(b"data: 0123456789\n\ndata: ok\n\n");
    assert_eq!(out, [b"ok".to_vec()]);
    assert_eq!(parser.oversized(), 1);
    // A line with no terminator stops growing the buffer at the bound, and the
    // event it belongs to is counted as oversized — not truncated and parsed.
    let mut parser = SseParser::new(8);
    assert!(parser.push(&[b'x'; 10_000]).is_empty());
    assert!(parser.pending.len() <= 8 + super::LINE_OVERHEAD);
    assert_eq!(parser.push(b"\n\ndata: ok\n\n"), [b"ok".to_vec()]);
    assert_eq!(parser.oversized(), 1);
}

#[test]
fn a_line_of_exactly_the_limit_is_kept() {
    // Within one chunk…
    let mut splitter = LineSplitter::new(4);
    assert_eq!(splitter.push(b"abcd\n"), [Line::Complete(b"abcd".to_vec())]);
    // …split across chunks…
    let mut splitter = LineSplitter::new(4);
    assert!(splitter.push(b"ab").is_empty());
    assert_eq!(splitter.push(b"cd\n"), [Line::Complete(b"abcd".to_vec())]);
    // …and unterminated at end of stream.
    let mut splitter = LineSplitter::new(4);
    assert!(splitter.push(b"abcd").is_empty());
    assert_eq!(splitter.finish(), Some(Line::Complete(b"abcd".to_vec())));
    // One byte more is oversized in each case.
    let mut splitter = LineSplitter::new(4);
    assert_eq!(splitter.push(b"abcde\n"), [Line::Oversized]);
}

#[test]
fn an_sse_event_of_exactly_the_limit_is_kept() {
    let mut parser = SseParser::new(4);
    assert_eq!(parser.push(b"data: abcd\n\n"), [b"abcd".to_vec()]);
    // Two data lines joined by `\n` count the separator.
    let mut parser = SseParser::new(4);
    assert_eq!(parser.push(b"data: a\ndata: bc\n\n"), [b"a\nbc".to_vec()]);
    let mut parser = SseParser::new(4);
    assert!(parser.push(b"data: ab\ndata: cd\n\n").is_empty());
    assert_eq!(parser.oversized(), 1);
}
