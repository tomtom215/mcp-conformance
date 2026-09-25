// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Incremental SSE frame parsing for the session tap.

/// Incremental SSE frame splitter: feed byte chunks, get the JSON payloads
/// of completed `data:` frames. Carries partial frames across chunks.
///
/// Framing is done on bytes and text is decoded per complete frame, so a
/// multi-byte character split across two chunks — which a network may do to
/// any non-ASCII stream — is reassembled before it is decoded. A frame that is
/// not UTF-8 is skipped on its own: frame boundaries are byte sequences, so one
/// bad frame cannot desynchronize the ones after it.
#[derive(Default)]
pub(super) struct SseSplitter {
    buffer: Vec<u8>,
    /// Set once an un-delimited frame outgrew the recording budget. The stream
    /// keeps flowing to the client; the tap stops parsing it, loudly.
    stopped: bool,
}

impl SseSplitter {
    /// Consumes one chunk and returns the payloads of every frame it completed.
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<serde_json::Value> {
        if self.stopped {
            return Vec::new();
        }
        self.buffer.extend_from_slice(chunk);
        let mut payloads = Vec::new();
        let mut rest: &[u8] = &self.buffer;
        // The iteration bound is a real invariant, not decoration: every
        // completed frame consumes at least its boundary bytes, so an n-byte
        // buffer holds at most n frames. Bounding the loop makes an infinite
        // spin impossible even if frame-splitting were to stop consuming
        // input — a recording bug must never wedge the serving path.
        for _ in 0..=self.buffer.len() {
            let Some((frame, next)) = split_frame(rest) else {
                break;
            };
            if let Some(payload) = payload(frame) {
                payloads.push(payload);
            }
            rest = &rest[next..];
        }
        let consumed = self.buffer.len() - rest.len();
        self.buffer.drain(..consumed);
        // The JSON path bounds recorded bodies (MAX_RECORDED_BODY); without
        // the same bound here, one frame-boundary-free stream would grow
        // this buffer until the process dies. Recording is diagnostics — it
        // must never be the thing that takes the server down. The bound is
        // checked on the *residual* (after frame extraction), so any frame
        // up to the budget itself still records.
        if self.buffer.len() > super::MAX_RECORDED_BODY {
            self.stopped = true;
            self.buffer = Vec::new();
            eprintln!(
                "mcp-everything-server: tap stopped recording an SSE stream whose \
                 frame exceeded the recording budget"
            );
        }
        payloads
    }
}

/// The JSON payload of one frame's `data:` lines, if it has one.
fn payload(frame: &[u8]) -> Option<serde_json::Value> {
    let Ok(text) = std::str::from_utf8(frame) else {
        eprintln!("mcp-everything-server: tap skipped an SSE frame that is not UTF-8");
        return None;
    };
    let data = text
        .lines()
        .filter_map(|line| {
            line.strip_prefix("data:")
                .map(|d| d.strip_prefix(' ').unwrap_or(d))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return None;
    }
    serde_json::from_str(&data).ok()
}

/// Finds the first SSE frame boundary (`\n\n` or `\r\n\r\n`) in `buffer`,
/// returning the frame and the offset just past the boundary.
fn split_frame(buffer: &[u8]) -> Option<(&[u8], usize)> {
    let lf = find(buffer, b"\n\n").map(|i| (i, 2));
    let crlf = find(buffer, b"\r\n\r\n").map(|i| (i, 4));
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
    Some((&buffer[..index], index + width))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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

    /// `split_frame` as `(frame, remainder)` strings, for readable assertions.
    fn split(buffer: &str) -> Option<(String, String)> {
        split_frame(buffer.as_bytes()).map(|(frame, next)| {
            (
                String::from_utf8(frame.to_vec()).unwrap(),
                buffer[next..].to_owned(),
            )
        })
    }

    #[test]
    fn split_frame_returns_exact_frame_and_remainder() {
        assert_eq!(split("no boundary yet"), None);
        assert_eq!(
            split("data: 1\n\nrest"),
            Some(("data: 1".to_owned(), "rest".to_owned()))
        );
        assert_eq!(
            split("data: 1\r\n\r\nrest"),
            Some(("data: 1".to_owned(), "rest".to_owned()))
        );
        // An empty frame is still a frame: the boundary alone splits.
        assert_eq!(split("\n\ntail"), Some((String::new(), "tail".to_owned())));
    }

    #[test]
    fn split_frame_takes_the_earlier_boundary_when_both_framings_appear() {
        // CRLF boundary first: it must win even though an LF boundary follows.
        assert_eq!(
            split("a\r\n\r\nb\n\nc"),
            Some(("a".to_owned(), "b\n\nc".to_owned()))
        );
        // LF boundary first: it must win even though a CRLF boundary follows.
        assert_eq!(
            split("a\n\nb\r\n\r\nc"),
            Some(("a".to_owned(), "b\r\n\r\nc".to_owned()))
        );
        // A CRLF boundary consumes all four bytes: the frame carries no
        // trailing carriage return and the remainder starts after the
        // boundary, even at end of input.
        assert_eq!(split("x\r\n\r\n"), Some(("x".to_owned(), String::new())));
    }

    #[test]
    fn a_character_split_across_chunks_is_reassembled() {
        // "é" is 0xC3 0xA9: a network may deliver the two bytes separately.
        let frame = "data: {\"text\":\"é日本\"}\n\n".as_bytes();
        let cut = frame.iter().position(|&byte| byte == 0xC3).unwrap() + 1;
        let mut splitter = SseSplitter::default();
        assert!(splitter.push(&frame[..cut]).is_empty());
        assert_eq!(splitter.push(&frame[cut..]), vec![json!({"text": "é日本"})]);
        // Every split point, not just that one.
        for cut in 1..frame.len() {
            let mut splitter = SseSplitter::default();
            let mut got = splitter.push(&frame[..cut]);
            got.extend(splitter.push(&frame[cut..]));
            assert_eq!(got, vec![json!({"text": "é日本"})], "cut at {cut}");
        }
    }

    #[test]
    fn a_frame_that_is_not_utf8_is_skipped_and_the_next_still_records() {
        let mut splitter = SseSplitter::default();
        let mut stream = b"data: {\"a\":1}\n\ndata: \xFF\xFE\n\n".to_vec();
        stream.extend_from_slice(b"data: {\"b\":2}\n\n");
        assert_eq!(
            splitter.push(&stream),
            vec![json!({"a": 1}), json!({"b": 2})]
        );
        assert!(!splitter.stopped);
    }

    #[test]
    fn splitter_stops_recording_when_a_frame_outgrows_the_budget() {
        let mut splitter = SseSplitter::default();
        // One giant chunk with no frame boundary anywhere.
        let oversized = "data: ".to_owned() + &"x".repeat(super::super::MAX_RECORDED_BODY + 1);
        assert!(splitter.push(oversized.as_bytes()).is_empty());
        // The buffer is freed, not merely cleared-but-capacity-retained.
        assert_eq!(splitter.buffer.capacity(), 0);
        assert!(splitter.stopped);
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
        assert!(!splitter.stopped, "exactly-at-budget must not overflow");
        assert_eq!(splitter.push(b"\n\n"), vec![json!(body)]);
    }
}
