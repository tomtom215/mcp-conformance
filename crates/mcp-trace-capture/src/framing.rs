// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Message framing for the two transports: newline-delimited JSON for stdio, and
//! Server-Sent Events for streamable HTTP.
//!
//! Both work on bytes and decode only complete units (a line, an event), so a network
//! read or pipe read that splits a multi-byte UTF-8 character — or a `\r\n` pair —
//! across two chunks cannot change what is recorded.

use serde_json::Value;

mod faithful;

/// Splits a byte stream into newline-terminated lines.
///
/// The stdio transport delimits messages with `\n`; a trailing `\r` is tolerated and
/// removed, and does not count toward the limit. A line longer than `max_line` is
/// not buffered further: it is reported as oversized and discarded up to its
/// newline, so a runaway writer cannot exhaust the capture's memory. The bytes
/// themselves are forwarded by the caller regardless.
#[derive(Debug)]
pub struct LineSplitter {
    buffer: Vec<u8>,
    max_line: usize,
    discarding: bool,
}

/// One unit the splitter produced.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Line {
    /// A complete line, without its terminator.
    Complete(Vec<u8>),
    /// A line that exceeded the limit; its content was not kept.
    Oversized,
    /// What the stream ended with after its last newline, without a trailing
    /// `\r`: stdio framing never completed it, so no peer received it as a
    /// message — not even when it is JSON.
    Unterminated(Vec<u8>),
}

impl LineSplitter {
    /// A splitter keeping lines up to `max_line` bytes.
    #[must_use]
    pub const fn new(max_line: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_line,
            discarding: false,
        }
    }

    /// Consumes a chunk and returns every line it completed.
    pub fn push(&mut self, mut chunk: &[u8]) -> Vec<Line> {
        let mut lines = Vec::new();
        while let Some(end) = chunk.iter().position(|&byte| byte == b'\n') {
            let (head, rest) = chunk.split_at(end);
            chunk = &rest[1..];
            if std::mem::take(&mut self.discarding) {
                lines.push(Line::Oversized);
                continue;
            }
            let carriage_return = head
                .last()
                .or_else(|| self.buffer.last())
                .is_some_and(|&byte| byte == b'\r');
            let length = self.buffer.len() + head.len() - usize::from(carriage_return);
            if length > self.max_line {
                self.buffer.clear();
                lines.push(Line::Oversized);
                continue;
            }
            self.buffer.extend_from_slice(head);
            let mut line = std::mem::take(&mut self.buffer);
            if carriage_return {
                line.pop();
            }
            lines.push(Line::Complete(line));
        }
        // One byte of room beyond the limit: a line of exactly the limit may still
        // be waiting for the `\n` of its `\r\n`.
        if !self.discarding {
            if self.buffer.len() + chunk.len() > self.max_line.saturating_add(1) {
                self.buffer.clear();
                self.discarding = true;
            } else {
                self.buffer.extend_from_slice(chunk);
            }
        }
        lines
    }

    /// The unterminated remainder at end of stream, if any: [`Line::Unterminated`],
    /// or [`Line::Oversized`] past the limit.
    pub fn finish(&mut self) -> Option<Line> {
        if std::mem::take(&mut self.discarding) {
            return Some(Line::Oversized);
        }
        let mut line = std::mem::take(&mut self.buffer);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.len() > self.max_line {
            return Some(Line::Oversized);
        }
        (!line.is_empty()).then_some(Line::Unterminated(line))
    }
}

/// Parses a line as a JSON value; `None` for anything else, including a blank line.
#[must_use]
pub fn parse_json(bytes: &[u8]) -> Option<Value> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    serde_json::from_slice(bytes).ok()
}

/// What a message's bytes are, for recording.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Parsed {
    /// JSON whose parsed value is exactly what the bytes say.
    Json(Value),
    /// JSON the trace cannot hold as sent: a number a double cannot represent
    /// exactly (an integer past 64 bits, say), or a member name repeated in an
    /// object. The validator reads a trace's payloads as parsed JSON too, so no
    /// recorded value could say what was sent; it is recorded as a transport's
    /// non-JSON is.
    Altered,
    /// Not JSON, including a blank line.
    NotJson,
}

/// Parses a message for recording: [`parse_json`], and whether the value it
/// gives is faithful to the bytes.
///
/// The faithfulness scan runs first, so what it holds — the member names of
/// each open object — is gone before the parsed value exists: held together,
/// the two were the peak of recording a large object (measured at 16 MiB of
/// short members: 302 MiB with the value first, 190 MiB this way). The scan
/// takes any bytes; a line that is not JSON is classified by the parse as
/// before, and the outcome is the same for every input.
#[must_use]
pub fn parse_message(bytes: &[u8]) -> Parsed {
    if !faithful::is_faithful(bytes) {
        return if parse_json(bytes).is_some() {
            Parsed::Altered
        } else {
            Parsed::NotJson
        };
    }
    parse_json(bytes).map_or(Parsed::NotJson, Parsed::Json)
}

/// An incremental Server-Sent Events parser (WHATWG HTML §9.2.6).
///
/// Lines end at CRLF, LF, or a lone CR; `data` fields accumulate joined by `\n`; a
/// blank line dispatches; `:` lines are comments; a leading BOM is skipped. Only
/// `data` is kept — the trace records the JSON-RPC messages a stream carries, not its
/// event ids or retry hints — and only from events of type `message` (no `event`
/// field, or `event: message`): MCP clients, the official SDKs' among them, act on
/// no other, so an event of another type is dropped and counted
/// ([`SseParser::other_types`]). An event whose data, or any one of its lines,
/// exceeds `max_event` bytes is dropped and counted ([`SseParser::oversized`])
/// rather than buffered without bound.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent parts of the parse state: where a line ends, whether the \
              stream has started, the event's type, and whether a line is skipped"
)]
pub struct SseParser {
    pending: Vec<u8>,
    event: EventData,
    started: bool,
    skip_lf: bool,
    max_event: usize,
    oversized: u64,
    /// The event being assembled has an `event` type other than `message`.
    other_type: bool,
    other_types: u64,
    /// The rest of the current line belongs to an event [`SseParser::drop_event`]
    /// dropped: skipped up to its end.
    skip_line: bool,
}

/// Room a line needs beyond its data: the field name and `": "` of the longest
/// field this parser reads (`data: ` is six bytes). The event limit applies to the
/// data itself; this only keeps an unterminated line from growing without bound.
const LINE_OVERHEAD: usize = 16;

/// The data of the event being assembled.
#[derive(Debug)]
enum EventData {
    /// No `data` field yet.
    Empty,
    /// The joined `data` fields so far.
    Data(Vec<u8>),
    /// Over the limit; discarded until the event ends.
    Oversized,
    /// Dropped by [`SseParser::drop_event`]; discarded until the event ends.
    Dropped,
}

impl SseParser {
    /// A parser keeping event data up to `max_event` bytes.
    #[must_use]
    pub const fn new(max_event: usize) -> Self {
        Self {
            pending: Vec::new(),
            event: EventData::Empty,
            started: false,
            skip_lf: false,
            max_event,
            oversized: 0,
            other_type: false,
            other_types: 0,
            skip_line: false,
        }
    }

    /// Consumes a chunk and returns the `data` of every event it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut events = Vec::new();
        for &byte in chunk {
            if std::mem::take(&mut self.skip_lf) && byte == b'\n' {
                continue;
            }
            match byte {
                b'\r' => {
                    self.skip_lf = true;
                    self.end_line(&mut events);
                }
                b'\n' => self.end_line(&mut events),
                _ if self.skip_line => {}
                _ if self.pending.len() < self.max_event.saturating_add(LINE_OVERHEAD) => {
                    self.pending.push(byte);
                }
                // A line longer than any event may be: the event it belongs to is
                // oversized, and the line stops growing.
                _ if matches!(self.event, EventData::Dropped) => {}
                _ => self.event = EventData::Oversized,
            }
        }
        events
    }

    /// How many bytes the parser holds: the event being assembled, and the
    /// line being read.
    #[must_use]
    pub const fn buffered(&self) -> usize {
        let data = match &self.event {
            EventData::Data(data) => data.len(),
            EventData::Empty | EventData::Oversized | EventData::Dropped => 0,
        };
        data + self.pending.len()
    }

    /// Drops the event being assembled, and the rest of the line being read:
    /// the caller could not afford to hold them. The event is not reported,
    /// and not counted here; its bytes were the caller's to relay.
    pub fn drop_event(&mut self) {
        self.skip_line = !self.pending.is_empty();
        self.pending = Vec::new();
        self.event = EventData::Dropped;
    }

    /// Events dropped for exceeding the limit.
    #[must_use]
    pub const fn oversized(&self) -> u64 {
        self.oversized
    }

    /// Events dropped for a type other than `message`.
    #[must_use]
    pub const fn other_types(&self) -> u64 {
        self.other_types
    }

    fn end_line(&mut self, events: &mut Vec<Vec<u8>>) {
        if std::mem::take(&mut self.skip_line) {
            return;
        }
        let mut line = std::mem::take(&mut self.pending);
        if !std::mem::replace(&mut self.started, true) && line.starts_with(&[0xEF, 0xBB, 0xBF]) {
            line.drain(..3);
        }
        if line.is_empty() {
            self.dispatch(events);
            return;
        }
        if line.first() == Some(&b':') {
            return;
        }
        let (field, value) = line.iter().position(|&byte| byte == b':').map_or(
            (line.as_slice(), &[][..]),
            |colon| {
                let value = &line[colon + 1..];
                (&line[..colon], value.strip_prefix(b" ").unwrap_or(value))
            },
        );
        if field == b"event" {
            // An empty type is the default, `message`.
            self.other_type = !value.is_empty() && value != b"message";
            return;
        }
        if field != b"data" {
            return;
        }
        let limit = self.max_event;
        self.event = match std::mem::replace(&mut self.event, EventData::Empty) {
            EventData::Dropped => EventData::Dropped,
            EventData::Empty if value.len() <= limit => EventData::Data(value.to_vec()),
            EventData::Data(mut data) if data.len() + 1 + value.len() <= limit => {
                data.push(b'\n');
                data.extend_from_slice(value);
                EventData::Data(data)
            }
            _ => EventData::Oversized,
        };
    }

    fn dispatch(&mut self, events: &mut Vec<Vec<u8>>) {
        let other_type = std::mem::take(&mut self.other_type);
        match std::mem::replace(&mut self.event, EventData::Empty) {
            EventData::Oversized => self.oversized += 1,
            EventData::Data(data) if !data.is_empty() && other_type => self.other_types += 1,
            EventData::Data(data) if !data.is_empty() => events.push(data),
            EventData::Data(_) | EventData::Empty | EventData::Dropped => {}
        }
    }
}

#[cfg(test)]
mod tests;
