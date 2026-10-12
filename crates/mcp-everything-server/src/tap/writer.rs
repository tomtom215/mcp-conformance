// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The tap's writer task: the single consumer of recorded events, sequencing
//! each per file so the trace schema's strictly-increasing `seq` holds by
//! construction, appending one JSON line, and flushing before the next.

use std::collections::HashMap;
use std::path::PathBuf;

use mcp_conformance_core::trace::{TraceEvent, TransportKind};
use tokio::io::AsyncWriteExt as _;

use super::Record;

/// The writer task: sequences each record's events per file (the schema's
/// strictly-increasing rule holds by construction), appends them as adjacent
/// JSON lines in one write, and flushes before accepting the next record —
/// everything enqueued before a kill is durable.
///
/// Each record opens its file for append and closes it again, so a long-lived
/// server holds no descriptor per session it has ever seen; only each file's
/// next `seq` is kept. That was an open handle per session until 0.6.0, which a
/// server run for days against many clients would have exhausted.
pub(super) async fn write_loop(mut receiver: tokio::sync::mpsc::Receiver<Record>) {
    let mut next_seq: HashMap<PathBuf, u64> = HashMap::new();
    while let Some(record) = receiver.recv().await {
        let path = &record.file.path;
        let first = next_seq.get(path).copied().unwrap_or(0);
        // The record's events become adjacent lines, written in one append.
        let mut lines = Vec::new();
        let mut seq = first;
        for (direction, body) in record.events {
            let event = TraceEvent::new(seq, direction, TransportKind::StreamableHttp, body);
            let Ok(line) = serde_json::to_vec(&event) else {
                eprintln!("mcp-everything-server: tap event unserializable; skipped");
                continue;
            };
            lines.extend_from_slice(&line);
            lines.push(b'\n');
            seq += 1;
        }
        if seq == first {
            continue;
        }
        let line = lines;
        let write = async {
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await?;
            file.write_all(&line).await?;
            file.flush().await
        };
        match write.await {
            // The seq is spent only once its line is written, so a failed
            // write leaves no gap for the next record to straddle.
            Ok(()) => {
                next_seq.insert(path.clone(), seq);
            }
            Err(error) => eprintln!(
                "mcp-everything-server: tap write to {} failed: {error}",
                path.display()
            ),
        }
    }
}
