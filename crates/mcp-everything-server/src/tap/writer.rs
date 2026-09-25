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

/// The writer task: sequences each record per file (the schema's
/// strictly-increasing rule holds by construction), appends it as one JSON
/// line, and flushes before accepting the next — everything enqueued before
/// a kill is durable.
///
/// Each record opens its file for append and closes it again, so a long-lived
/// server holds no descriptor per session it has ever seen; only each file's
/// next `seq` is kept. That was an open handle per session until 0.6.0, which a
/// server run for days against many clients would have exhausted.
pub(super) async fn write_loop(mut receiver: tokio::sync::mpsc::Receiver<Record>) {
    let mut next_seq: HashMap<PathBuf, u64> = HashMap::new();
    while let Some(record) = receiver.recv().await {
        let path = &record.file.path;
        let seq = next_seq.get(path).copied().unwrap_or(0);
        let event = TraceEvent::new(
            seq,
            record.direction,
            TransportKind::StreamableHttp,
            record.body,
        );
        let Ok(mut line) = serde_json::to_vec(&event) else {
            eprintln!("mcp-everything-server: tap event unserializable; skipped");
            continue;
        };
        line.push(b'\n');
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
                next_seq.insert(path.clone(), seq + 1);
            }
            Err(error) => eprintln!(
                "mcp-everything-server: tap write to {} failed: {error}",
                path.display()
            ),
        }
    }
}
