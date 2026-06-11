// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Incremental SSE frame parsing for the session tap.

/// Incremental SSE frame splitter: feed byte chunks, get the JSON payloads
/// of completed `data:` frames. Carries partial frames across chunks.
#[derive(Default)]
pub(super) struct SseSplitter {
    buffer: String,
    /// Set once an un-delimited frame outgrows the recording budget: the
    /// stream keeps flowing to the client, but this tap stops parsing it.
    overflowed: bool,
}

impl SseSplitter {
    /// Consumes one chunk and returns the payloads of every frame it
    /// completed. Non-UTF-8 chunks abort recording for this stream (the
    /// bytes still flow to the client untouched).
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<serde_json::Value> {
        if self.overflowed {
            return Vec::new();
        }
        let Ok(text) = std::str::from_utf8(chunk) else {
            self.buffer.clear();
            return Vec::new();
        };
        self.buffer.push_str(text);
        let mut payloads = Vec::new();
        // SSE events end at a blank line; tolerate both LF and CRLF framing.
        // The iteration bound is a real invariant, not decoration: every
        // completed frame consumes at least its boundary bytes, so an n-byte
        // buffer holds at most n frames. Bounding the loop makes an infinite
        // spin impossible even if frame-splitting were to stop consuming
        // input — a recording bug must never wedge the serving path.
        for _ in 0..=self.buffer.len() {
            let Some((frame, rest)) = split_frame(&self.buffer) else {
                break;
            };
            let data = frame
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|d| d.strip_prefix(' ').unwrap_or(d))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty()
                && let Ok(payload) = serde_json::from_str(&data)
            {
                payloads.push(payload);
            }
            self.buffer = rest;
        }
        // The JSON path bounds recorded bodies (MAX_RECORDED_BODY); without
        // the same bound here, one frame-boundary-free stream would grow
        // this buffer until the process dies. Recording is diagnostics — it
        // must never be the thing that takes the server down. The bound is
        // checked on the *residual* (after frame extraction), so any frame
        // up to the budget itself still records; the buffer can transiently
        // hold residual-plus-one-chunk, which network reads keep small.
        if self.buffer.len() > super::MAX_RECORDED_BODY {
            self.overflowed = true;
            self.buffer = String::new();
            eprintln!(
                "mcp-everything-server: tap stopped recording an SSE stream whose \
                 frame exceeded the recording budget"
            );
        }
        payloads
    }
}

/// Splits `buffer` at the first SSE frame boundary (`\n\n` or `\r\n\r\n`),
/// returning the frame and the remainder.
fn split_frame(buffer: &str) -> Option<(String, String)> {
    let lf = buffer.find("\n\n").map(|i| (i, 2));
    let crlf = buffer.find("\r\n\r\n").map(|i| (i, 4));
    let (index, width) = match (lf, crlf) {
        (Some((li, lw)), Some((ci, cw))) => {
            if ci < li {
                (ci, cw)
            } else {
                (li, lw)
            }
        }
        (Some(found), None) | (None, Some(found)) => found,
        (None, None) => return None,
    };
    Some((
        buffer[..index].to_owned(),
        buffer[index + width..].to_owned(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn splitter_yields_each_completed_frame_and_carries_partials() {
        let mut splitter = SseSplitter::default();
        assert!(splitter.push(b"data: {\"a\":").is_empty());
        let got = splitter.push(b"1}\n\ndata: {\"b\":2}\n\ndata: {\"c\"");
        assert_eq!(got, vec![json!({"a": 1}), json!({"b": 2})]);
        assert_eq!(splitter.push(b":3}\n\n"), vec![json!({"c": 3})]);
    }

    #[test]
    fn splitter_joins_multi_line_data_and_tolerates_crlf() {
        let mut splitter = SseSplitter::default();
        let got = splitter.push(b"event: message\r\ndata: [1,\r\ndata: 2]\r\n\r\n");
        assert_eq!(got, vec![json!([1, 2])]);
    }

    #[test]
    fn splitter_ignores_non_json_and_empty_frames() {
        let mut splitter = SseSplitter::default();
        assert!(splitter.push(b": keep-alive\n\n").is_empty());
        assert!(splitter.push(b"data: not json\n\n").is_empty());
        assert_eq!(splitter.push(b"data: 7\n\n"), vec![json!(7)]);
    }

    #[test]
    fn split_frame_returns_exact_frame_and_remainder() {
        assert_eq!(split_frame("no boundary yet"), None);
        assert_eq!(
            split_frame("data: 1\n\nrest"),
            Some(("data: 1".to_owned(), "rest".to_owned()))
        );
        assert_eq!(
            split_frame("data: 1\r\n\r\nrest"),
            Some(("data: 1".to_owned(), "rest".to_owned()))
        );
        // An empty frame is still a frame: the boundary alone splits.
        assert_eq!(
            split_frame("\n\ntail"),
            Some((String::new(), "tail".to_owned()))
        );
    }

    #[test]
    fn split_frame_takes_the_earlier_boundary_when_both_framings_appear() {
        // CRLF boundary first: it must win even though an LF boundary follows.
        assert_eq!(
            split_frame("a\r\n\r\nb\n\nc"),
            Some(("a".to_owned(), "b\n\nc".to_owned()))
        );
        // LF boundary first: it must win even though a CRLF boundary follows.
        assert_eq!(
            split_frame("a\n\nb\r\n\r\nc"),
            Some(("a".to_owned(), "b\r\n\r\nc".to_owned()))
        );
        // A CRLF boundary consumes all four bytes: the frame carries no
        // trailing carriage return and the remainder starts after the
        // boundary, even at end of input.
        assert_eq!(
            split_frame("x\r\n\r\n"),
            Some(("x".to_owned(), String::new()))
        );
    }

    #[test]
    fn splitter_stops_recording_when_a_frame_outgrows_the_budget() {
        let mut splitter = SseSplitter::default();
        // One giant chunk with no frame boundary anywhere.
        let oversized = "data: ".to_owned() + &"x".repeat(super::super::MAX_RECORDED_BODY + 1);
        assert!(splitter.push(oversized.as_bytes()).is_empty());
        // The buffer is freed, not merely cleared-but-capacity-retained.
        assert_eq!(splitter.buffer.capacity(), 0);
        assert!(splitter.overflowed);
        // The stream is poisoned for recording: even well-formed frames that
        // follow are not parsed (the client still received every byte).
        let frame = b"data: {\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n\n";
        assert!(splitter.push(frame).is_empty());
    }

    #[test]
    fn splitter_budget_boundary_is_exclusive() {
        // Boundary pinning: a buffered frame of exactly the budget is within
        // it (> overflows, >= must not). The payload completes and records.
        let mut splitter = SseSplitter::default();
        let body = "x".repeat(super::super::MAX_RECORDED_BODY - 8);
        let at_budget = format!("data: \"{body}\"");
        assert_eq!(at_budget.len(), super::super::MAX_RECORDED_BODY);
        assert!(splitter.push(at_budget.as_bytes()).is_empty());
        assert!(!splitter.overflowed, "exactly-at-budget must not overflow");
        assert_eq!(splitter.push(b"\n\n"), vec![json!(body)]);
    }
}
