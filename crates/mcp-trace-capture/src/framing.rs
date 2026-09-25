// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Message framing for the two transports: newline-delimited JSON for stdio, and
//! Server-Sent Events for streamable HTTP.
//!
//! Both work on bytes and decode only complete units (a line, an event), so a network
//! read or pipe read that splits a multi-byte UTF-8 character — or a `\r\n` pair —
//! across two chunks cannot change what is recorded.

use serde_json::Value;

/// Splits a byte stream into newline-terminated lines.
///
/// The stdio transport delimits messages with `\n`; a trailing `\r` is tolerated and
/// removed. A line longer than `max_line` is not buffered further: it is reported as
/// oversized and discarded up to its newline, so a runaway writer cannot exhaust the
/// capture's memory. The bytes themselves are forwarded by the caller regardless.
#[derive(Debug)]
pub struct LineSplitter {
    buffer: Vec<u8>,
    max_line: usize,
    discarding: bool,
}

/// One unit the splitter produced.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// A complete line, without its terminator.
    Complete(Vec<u8>),
    /// A line that exceeded the limit; its content was not kept.
    Oversized,
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
            if self.discarding {
                self.discarding = false;
                lines.push(Line::Oversized);
                continue;
            }
            if self.buffer.len() + head.len() > self.max_line {
                self.buffer.clear();
                lines.push(Line::Oversized);
                continue;
            }
            self.buffer.extend_from_slice(head);
            let mut line = std::mem::take(&mut self.buffer);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            lines.push(Line::Complete(line));
        }
        if !self.discarding {
            if self.buffer.len() + chunk.len() > self.max_line {
                self.buffer.clear();
                self.discarding = true;
            } else {
                self.buffer.extend_from_slice(chunk);
            }
        }
        lines
    }

    /// The unterminated remainder at end of stream, if any.
    pub fn finish(&mut self) -> Option<Line> {
        if std::mem::take(&mut self.discarding) {
            return Some(Line::Oversized);
        }
        let mut line = std::mem::take(&mut self.buffer);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        (!line.is_empty()).then_some(Line::Complete(line))
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

/// An incremental Server-Sent Events parser (WHATWG HTML §9.2.6).
///
/// Lines end at CRLF, LF, or a lone CR; `data` fields accumulate joined by `\n`; a
/// blank line dispatches; `:` lines are comments; a leading BOM is skipped. Only
/// `data` is kept — the trace records the JSON-RPC messages a stream carries, not its
/// event ids or retry hints. An event whose data, or any one of its lines, exceeds
/// `max_event` bytes is dropped and counted ([`SseParser::oversized`]) rather than
/// buffered without bound.
#[derive(Debug)]
pub struct SseParser {
    pending: Vec<u8>,
    event: EventData,
    started: bool,
    skip_lf: bool,
    max_event: usize,
    oversized: u64,
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
                _ if self.pending.len() < self.max_event.saturating_add(LINE_OVERHEAD) => {
                    self.pending.push(byte);
                }
                // A line longer than any event may be: the event it belongs to is
                // oversized, and the line stops growing.
                _ => self.event = EventData::Oversized,
            }
        }
        events
    }

    /// Events dropped for exceeding the limit.
    #[must_use]
    pub const fn oversized(&self) -> u64 {
        self.oversized
    }

    fn end_line(&mut self, events: &mut Vec<Vec<u8>>) {
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
        if field != b"data" {
            return;
        }
        let limit = self.max_event;
        self.event = match std::mem::replace(&mut self.event, EventData::Empty) {
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
        match std::mem::replace(&mut self.event, EventData::Empty) {
            EventData::Oversized => self.oversized += 1,
            EventData::Data(data) if !data.is_empty() => events.push(data),
            EventData::Data(_) | EventData::Empty => {}
        }
    }
}

#[cfg(test)]
mod tests;
