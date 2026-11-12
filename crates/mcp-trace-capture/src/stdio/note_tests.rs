// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a stdio wrapper's closing event says it left out.

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::trace::{Direction, LifecycleEvent, TransportKind};
use tokio::io::AsyncReadExt as _;

use super::pump;
use super::tests::{Shared, bounded, events};
use crate::recorder::Recorder;

/// A line over the message limit is forwarded and not recorded; the trace's
/// closing event counts it, so the loss outlives the terminal.
#[tokio::test]
async fn a_line_over_the_message_limit_is_counted_in_the_closing_event() {
    bounded(async {
        let sink = Shared::default();
        let recorder = Recorder::new(sink.clone());
        let input = format!("{{\"pad\":\"{}\"}}\n{{\"ok\":1}}\n", "x".repeat(40));
        let (forward, mut forwarded) = tokio::io::duplex(1024);
        let unrecorded = pump(
            &recorder,
            Direction::ServerToClient,
            input.as_bytes(),
            forward,
            16,
        )
        .await
        .unwrap();
        let mut out = Vec::new();
        forwarded.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, input.as_bytes());
        assert_eq!(unrecorded.oversized, 1);
        recorder
            .close_with_note(
                Direction::ServerToClient,
                TransportKind::Stdio,
                LifecycleEvent::TransportClose,
            )
            .unwrap();
        let written = events(&sink);
        assert_eq!(written.len(), 2, "the short line and the close");
        assert_eq!(
            written[1].capture.as_ref().unwrap().unrecorded,
            std::collections::BTreeMap::from([("oversized".to_owned(), 1)])
        );
    })
    .await;
}

/// A line the recorder refuses for length is counted by the recorder itself:
/// the pump must not count it a second time.
#[tokio::test]
async fn a_line_refused_for_length_is_counted_once_in_the_closing_event() {
    bounded(async {
        let sink = Shared::default();
        // Room for the closing event and its note, not for the line.
        let recorder = Recorder::with_max_line(sink.clone(), 180);
        let input = format!("[{}]\n", ["9e15"; 10].join(","));
        let (forward, mut forwarded) = tokio::io::duplex(1024);
        let unrecorded = pump(
            &recorder,
            Direction::ServerToClient,
            input.as_bytes(),
            forward,
            1024,
        )
        .await
        .unwrap();
        forwarded.read_to_end(&mut Vec::new()).await.unwrap();
        assert_eq!(unrecorded.oversized, 1);
        recorder
            .close_with_note(
                Direction::ServerToClient,
                TransportKind::Stdio,
                LifecycleEvent::TransportClose,
            )
            .unwrap();
        let note = events(&sink).pop().unwrap().capture.unwrap();
        assert_eq!(note.unrecorded.get("oversized"), Some(&1), "{note:?}");
    })
    .await;
}
