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

/// Open handle plus the next sequence number to assign for one trace file.
struct FileState {
    file: tokio::fs::File,
    next_seq: u64,
}

/// The writer task: sequences each record per file (the schema's
/// strictly-increasing rule holds by construction), appends it as one JSON
/// line, and flushes before accepting the next — everything enqueued before
/// a kill is durable.
pub(super) async fn write_loop(mut receiver: tokio::sync::mpsc::Receiver<Record>) {
    let mut files: HashMap<PathBuf, FileState> = HashMap::new();
    while let Some(record) = receiver.recv().await {
        let path = &record.file.path;
        if !files.contains_key(path) {
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await
            {
                Ok(file) => {
                    files.insert(path.clone(), FileState { file, next_seq: 0 });
                }
                Err(error) => {
                    eprintln!(
                        "mcp-everything-server: tap cannot open {}: {error}",
                        path.display()
                    );
                    continue;
                }
            }
        }
        if let Some(state) = files.get_mut(path) {
            let event = TraceEvent::new(
                state.next_seq,
                record.direction,
                TransportKind::StreamableHttp,
                record.body,
            );
            let Ok(line) = serde_json::to_string(&event) else {
                eprintln!("mcp-everything-server: tap event unserializable; skipped");
                continue;
            };
            state.next_seq += 1;
            let write = async {
                state.file.write_all(line.as_bytes()).await?;
                state.file.write_all(b"\n").await?;
                state.file.flush().await
            };
            if let Err(error) = write.await {
                eprintln!(
                    "mcp-everything-server: tap write to {} failed: {error}",
                    path.display()
                );
            }
        }
    }
}
