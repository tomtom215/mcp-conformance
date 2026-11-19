// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Server-sent events, as far as a client of the Streamable HTTP transport
//! reads them: the `data` of each event, in order.
//!
//! Comment lines (keep-alives) and the `event`, `id` and `retry` fields carry
//! no message and are skipped. Lines end with LF, CRLF or a lone CR, as the
//! event-stream format allows.

/// An event stream being read chunk by chunk.
#[derive(Debug, Default)]
pub(crate) struct Events {
    /// Bytes of a line not yet ended.
    line: Vec<u8>,
    /// The `data` lines of the event not yet dispatched.
    data: Vec<String>,
    /// The last byte fed was a CR, so an LF that follows ends the same line.
    after_cr: bool,
}

impl Events {
    /// Takes the next bytes of the stream; returns the `data` of each event
    /// they complete.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut complete = Vec::new();
        for &byte in chunk {
            let after_cr = std::mem::replace(&mut self.after_cr, byte == b'\r');
            match byte {
                b'\n' if after_cr => {}
                b'\n' | b'\r' => {
                    let line = std::mem::take(&mut self.line);
                    if let Some(event) = self.end_line(&line) {
                        complete.push(event);
                    }
                }
                other => self.line.push(other),
            }
        }
        complete
    }

    /// Takes one whole line; returns an event's data when the line is the
    /// blank one that ends it.
    fn end_line(&mut self, line: &[u8]) -> Option<String> {
        if line.is_empty() {
            let data = std::mem::take(&mut self.data);
            return (!data.is_empty()).then(|| data.join("\n"));
        }
        let line = String::from_utf8_lossy(line);
        if let Some(value) = line.strip_prefix("data:") {
            self.data
                .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
        } else if line == "data" {
            self.data.push(String::new());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::Events;

    const NONE: [String; 0] = [];

    #[test]
    fn data_is_dispatched_at_the_blank_line_whatever_the_line_ending() {
        for ending in ["\n", "\r\n", "\r"] {
            let mut events = Events::default();
            let stream = format!("event: message{ending}data: {{\"a\":1}}{ending}{ending}");
            assert_eq!(events.feed(stream.as_bytes()), [r#"{"a":1}"#], "{ending:?}");
        }
    }

    #[test]
    fn an_event_split_across_chunks_is_one_event() {
        let mut events = Events::default();
        assert_eq!(events.feed(b"data: {\"a\""), NONE);
        assert_eq!(events.feed(b":1}\r"), NONE);
        // The LF completing a CRLF arrives in the next chunk and ends nothing
        // twice; the blank line after it dispatches.
        assert_eq!(events.feed(b"\n\r\n"), [r#"{"a":1}"#]);
    }

    #[test]
    fn a_crlf_between_two_data_lines_ends_one_line_not_an_event() {
        // Were the LF of a CRLF taken for a line of its own, it would be the
        // blank line that dispatches, and this one event would arrive as two.
        let mut events = Events::default();
        assert_eq!(events.feed(b"data: 1\r\ndata: 2\r\n\r\n"), ["1\n2"]);
    }

    #[test]
    fn comments_and_other_fields_carry_no_data() {
        let mut events = Events::default();
        assert_eq!(events.feed(b": keep-alive\n\nid: 7\nretry: 5\n\n"), NONE);
    }

    #[test]
    fn several_data_lines_join_with_a_newline_and_one_leading_space_is_dropped() {
        let mut events = Events::default();
        assert_eq!(
            events.feed(b"data:first\ndata:  second\ndata\n\n"),
            ["first\n second\n"]
        );
    }

    #[test]
    fn two_events_in_one_chunk_come_out_in_order() {
        let mut events = Events::default();
        assert_eq!(events.feed(b"data: 1\n\ndata: 2\n\n"), ["1", "2"]);
    }
}
