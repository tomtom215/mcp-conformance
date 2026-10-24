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
            Line::Unterminated(b"{\"c\":1}".to_vec()),
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
    assert_eq!(splitter.push(b"too long"), Vec::<Line>::new());
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
    assert_eq!(
        parser.push(b"event: ping\n\nid: 3\n\ndata:\n\n"),
        Vec::<Vec<u8>>::new()
    );
}

#[test]
fn an_oversized_event_is_dropped_and_counted_without_affecting_the_next() {
    let mut parser = SseParser::new(8);
    assert_eq!(parser.push(b"data: ok\n\n"), [b"ok".to_vec()]);
    assert_eq!(parser.oversized(), 0);
    let out = parser.push(b"data: 0123456789\n\ndata: ok\n\n");
    assert_eq!(out, [b"ok".to_vec()]);
    assert_eq!(parser.oversized(), 1);
    // The count accumulates across events and chunks.
    assert_eq!(parser.push(b"data: 0123456789\n\n"), Vec::<Vec<u8>>::new());
    assert_eq!(parser.oversized(), 2);
    // A line with no terminator stops growing the buffer at the bound, and the
    // event it belongs to is counted as oversized — not truncated and parsed.
    let mut parser = SseParser::new(8);
    assert_eq!(parser.push(&[b'x'; 10_000]), Vec::<Vec<u8>>::new());
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
    assert_eq!(splitter.push(b"ab"), Vec::<Line>::new());
    assert_eq!(splitter.push(b"cd\n"), [Line::Complete(b"abcd".to_vec())]);
    // …and unterminated at end of stream.
    let mut splitter = LineSplitter::new(4);
    assert_eq!(splitter.push(b"abcd"), Vec::<Line>::new());
    assert_eq!(
        splitter.finish(),
        Some(Line::Unterminated(b"abcd".to_vec()))
    );
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
    assert_eq!(
        parser.push(b"data: ab\ndata: cd\n\n"),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(parser.oversized(), 1);
}

#[test]
fn only_a_bom_at_the_very_start_of_the_stream_is_skipped() {
    // At the start, the BOM is not part of the first field name.
    let mut parser = SseParser::new(1024);
    assert_eq!(
        parser.push("\u{FEFF}data: 1\n\n".as_bytes()),
        [b"1".to_vec()]
    );
    // Anywhere else it is, so this line's field is not `data` and is ignored.
    let mut parser = SseParser::new(1024);
    assert_eq!(
        parser.push("data: 1\n\n\u{FEFF}data: 2\n\n".as_bytes()),
        [b"1".to_vec()]
    );
}

/// The limit applies to the line, not its terminator: a `\r\n`-terminated line of
/// exactly the limit is kept wherever the input is cut, and one byte more is not.
#[test]
fn a_crlf_line_of_exactly_the_limit_is_kept() {
    let input = b"abcd\r\nabcde\r\nok\r\n";
    for cut in 0..=input.len() {
        let mut splitter = LineSplitter::new(4);
        let mut out = splitter.push(&input[..cut]);
        out.extend(splitter.push(&input[cut..]));
        out.extend(splitter.finish());
        assert_eq!(
            out,
            [
                Line::Complete(b"abcd".to_vec()),
                Line::Oversized,
                Line::Complete(b"ok".to_vec()),
            ],
            "cut at {cut}"
        );
    }
    // Unterminated at end of stream, with and without a stray `\r`.
    let mut splitter = LineSplitter::new(4);
    assert_eq!(splitter.push(b"abcd\r"), Vec::<Line>::new());
    assert_eq!(
        splitter.finish(),
        Some(Line::Unterminated(b"abcd".to_vec()))
    );
    let mut splitter = LineSplitter::new(4);
    assert_eq!(splitter.push(b"abcde"), Vec::<Line>::new());
    assert_eq!(splitter.finish(), Some(Line::Oversized));
}

/// Both official SDK clients act only on events of type `message` (the
/// default); an event of another type is no message to them, so it is not
/// recorded as one — and the type is reset with each event, data or not.
#[test]
fn only_message_events_are_kept_and_the_others_counted() {
    let mut parser = SseParser::new(1024);
    let events = parser.push(
        b"event: ping\ndata: {\"a\":1}\n\nevent: message\ndata: {\"b\":2}\n\n\
          event:\ndata: {\"c\":3}\n\ndata: {\"d\":4}\n\nevent: x\n\ndata: {\"e\":5}\n\n\
          event: endpoint\ndata: /messages\n\n",
    );
    assert_eq!(
        events,
        [
            b"{\"b\":2}".to_vec(),
            b"{\"c\":3}".to_vec(),
            b"{\"d\":4}".to_vec(),
            b"{\"e\":5}".to_vec(),
        ]
    );
    assert_eq!(parser.other_types(), 2);
    assert_eq!(parser.oversized(), 0);
}

#[test]
fn a_message_is_json_json_the_trace_would_alter_or_not_json() {
    assert_eq!(
        parse_message(b"{\"id\":1}"),
        Parsed::Json(serde_json::json!({"id": 1}))
    );
    assert_eq!(parse_message(b"{\"id\":1,\"id\":2}"), Parsed::Altered);
    assert_eq!(
        parse_message(b"{\"id\":123456789012345678901234567890}"),
        Parsed::Altered
    );
    assert_eq!(parse_message(b"not json"), Parsed::NotJson);
    assert_eq!(parse_message(b" "), Parsed::NotJson);
}

/// `parse_message` as it was before the faithfulness scan was moved ahead
/// of the parse: the parse first, the scan only on what parsed.
fn parse_message_value_first(bytes: &[u8]) -> Parsed {
    match parse_json(bytes) {
        Some(value) if faithful::is_faithful(bytes) => Parsed::Json(value),
        Some(_) => Parsed::Altered,
        None => Parsed::NotJson,
    }
}

/// The scan runs on bytes that may not be JSON at all, so it must neither
/// panic on them nor change what any message is recorded as: every input —
/// JSON of each kind, near-JSON, fragments cut anywhere, and bytes drawn from
/// a fixed generator — classifies as it did with the parse first.
#[test]
fn scanning_before_parsing_changes_no_classification() {
    let samples: Vec<Vec<u8>> = [
        &b""[..],
        b" ",
        b"{}",
        b"[]",
        b"\"\"",
        b"1",
        b"-",
        b"1e",
        b"\"",
        b"\"\\",
        b"{\"a\":",
        b"{\"a\":1,\"a\":2}",
        b"{\"id\":123456789012345678901234567890}",
        b"[1e400]",
        b"{\"a\":{\"b\":[1,{\"c\":\"\\\"d\"}]}}",
        b"{\"\\ud800\":1}",
        b"{\"a\":1}x",
        b"not json {\"a\":1,\"a\":2}",
        b"\xff\xfe{\"a\":1}",
        b"{\"\xff\":1}",
        b"[[[[[[[[[[1]]]]]]]]]]",
        b"{\"k\":\"v\",\"k\":\"v\"}",
        b"{\"k\":\"v\"},{\"k\":\"v\"}",
        b"\"a\"\"b\"",
        b"-0.0e-0",
        b"\"unterminated",
    ]
    .iter()
    .map(|sample| sample.to_vec())
    .collect();
    let mut inputs = samples.clone();
    for sample in &samples {
        for cut in 0..sample.len() {
            inputs.push(sample[..cut].to_vec());
            inputs.push(sample[cut..].to_vec());
        }
    }
    // Bytes from a fixed generator, over the characters JSON is made of.
    let alphabet = b"{}[]\":,.-+eE0123456789abn \\\xff";
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    for length in 0..64 {
        let mut bytes = Vec::with_capacity(length);
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let index = usize::try_from(state % alphabet.len() as u64).unwrap();
            bytes.push(alphabet[index]);
        }
        inputs.push(bytes);
    }
    for input in &inputs {
        assert_eq!(
            parse_message(input),
            parse_message_value_first(input),
            "{}",
            String::from_utf8_lossy(input)
        );
    }
}

/// What the parser holds is what the proxy's buffer budget pays for; an event
/// the budget cannot cover is dropped, mid-line or between lines, and the
/// parser picks up cleanly at the next event.
#[test]
fn a_dropped_event_is_skipped_to_its_end_and_the_next_is_kept() {
    let mut parser = SseParser::new(1024);
    assert_eq!(parser.push(b"data: {\"a\":"), Vec::<Vec<u8>>::new());
    assert_eq!(parser.buffered(), 11);
    parser.drop_event();
    assert_eq!(parser.buffered(), 0);
    assert_eq!(
        parser.push(b"1}\ndata: more\n\ndata: {\"b\":2}\n\n"),
        [b"{\"b\":2}".to_vec()]
    );
    // Dropped between two lines of its data.
    assert_eq!(parser.push(b"data: x\n"), Vec::<Vec<u8>>::new());
    assert_eq!(parser.buffered(), 1);
    parser.drop_event();
    assert_eq!(
        parser.push(b"data: yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\n\ndata: z\n\n"),
        [b"z".to_vec()]
    );
    assert_eq!((parser.oversized(), parser.other_types()), (0, 0));
}

/// The end of a dropped event is found exactly: the blank line right after
/// it, whether it was dropped mid-line or between lines, and however long
/// its remaining lines are.
#[test]
fn a_dropped_event_ends_at_its_own_blank_line() {
    // Mid-line: the rest of the line is skipped, not held, and the blank line
    // after it ends the event.
    let mut parser = SseParser::new(1024);
    parser.push(b"data: {\"a\":");
    parser.drop_event();
    assert_eq!(parser.push(b"1}"), Vec::<Vec<u8>>::new());
    assert_eq!(parser.buffered(), 0);
    assert_eq!(
        parser.push(b"\n\ndata: {\"b\":2}\n\n"),
        [b"{\"b\":2}".to_vec()]
    );
    // Between lines: the very next line is blank, and ends it.
    assert_eq!(parser.push(b"data: x\n"), Vec::<Vec<u8>>::new());
    parser.drop_event();
    assert_eq!(parser.push(b"\ndata: z\n\n"), [b"z".to_vec()]);
    // A line of a dropped event too long for any event is skipped too: the
    // event was dropped, not oversized.
    let mut small = SseParser::new(4);
    small.push(b"data: x\n");
    small.drop_event();
    let long = format!("data: {}\n\ndata: ok\n\n", "y".repeat(64));
    assert_eq!(small.push(long.as_bytes()), [b"ok".to_vec()]);
    assert_eq!((small.oversized(), small.other_types()), (0, 0));
}
