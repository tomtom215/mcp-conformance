// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Server-sent events, as far as a client of the Streamable HTTP transport
//! reads them: the `data` of each event, in order.
//!
//! Comment lines (keep-alives) and the `event` field carry no message and are
//! skipped. The last event's `id` and the stream's `retry` are kept: they are
//! what a client resumes a closed stream with (`Last-Event-ID`). Lines end
//! with LF, CRLF or a lone CR, as the event-stream format allows.

/// An event stream being read chunk by chunk.
#[derive(Debug, Default)]
pub(crate) struct Events {
    /// Bytes of a line not yet ended.
    line: Vec<u8>,
    /// The `data` lines of the event not yet dispatched.
    data: Vec<String>,
    /// The last byte fed was a CR, so an LF that follows ends the same line.
    after_cr: bool,
    /// The `id` of the event not yet dispatched.
    pending_id: Option<String>,
    /// The `id` of the last event dispatched that carried one.
    last_id: Option<String>,
    /// The reconnection time the server last asked for, in milliseconds.
    retry: Option<u64>,
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

    /// The `id` of the last complete event that carried one: what
    /// `Last-Event-ID` names when the stream is resumed.
    pub(crate) fn last_id(&self) -> Option<&str> {
        self.last_id.as_deref()
    }

    /// The reconnection time the server asked for (`retry`), in milliseconds.
    pub(crate) const fn retry(&self) -> Option<u64> {
        self.retry
    }

    /// Takes one whole line; returns an event's data when the line is the
    /// blank one that ends it.
    fn end_line(&mut self, line: &[u8]) -> Option<String> {
        if line.is_empty() {
            // An event with an id and no data (a priming event) still moves
            // the position a resumption starts from.
            if let Some(id) = self.pending_id.take() {
                self.last_id = Some(id);
            }
            let data = std::mem::take(&mut self.data);
            return (!data.is_empty()).then(|| data.join("\n"));
        }
        let line = String::from_utf8_lossy(line);
        if let Some(value) = line.strip_prefix("data:") {
            self.data
                .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
        } else if line == "data" {
            self.data.push(String::new());
        } else if let Some(value) = line.strip_prefix("id:") {
            self.pending_id = Some(value.strip_prefix(' ').unwrap_or(value).to_owned());
        } else if let Some(value) = line.strip_prefix("retry:") {
            // Digits only, as the format requires; anything else is ignored.
            if let Ok(retry) = value.trim_start_matches(' ').parse() {
                self.retry = Some(retry);
            }
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
    fn the_last_events_id_and_the_retry_time_are_kept_for_resuming() {
        let mut events = Events::default();
        assert_eq!((events.last_id(), events.retry()), (None, None));
        // An id is the event's only once the event is complete.
        assert_eq!(events.feed(b"id: 0/3\nretry: 500\n"), NONE);
        assert_eq!((events.last_id(), events.retry()), (None, Some(500)));
        // A priming event: an id and no data. It dispatches nothing and
        // still moves the position.
        assert_eq!(events.feed(b"data:\n\n"), [""]);
        assert_eq!(events.last_id(), Some("0/3"));
        assert_eq!(events.feed(b"id:1/3\ndata: {}\n\n"), ["{}"]);
        assert_eq!(events.last_id(), Some("1/3"));
        // An event without an id leaves the position; a retry that is not
        // digits is ignored.
        assert_eq!(events.feed(b"retry: soon\ndata: {}\n\n"), ["{}"]);
        assert_eq!((events.last_id(), events.retry()), (Some("1/3"), Some(500)));
        let mut bare = Events::default();
        assert_eq!(bare.feed(b"id: 7\n\n"), NONE);
        assert_eq!(bare.last_id(), Some("7"));
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
