// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The tap's writer task: the single consumer of recorded events, sequencing
//! each per file so the trace schema's strictly-increasing `seq` holds by
//! construction, appending one JSON line, and flushing before the next.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent, TransportKind};
use tokio::io::AsyncWriteExt as _;

use super::Record;
use super::files::SessionFile;

/// What the tap sends the writer, in the order it observed the exchanges.
pub(super) enum Command {
    /// Append the record's events to its file, behind any place reserved there.
    Write(Record),
    /// Reserve the next place in `file` for events known only later.
    Reserve { file: Arc<SessionFile>, place: u64 },
    /// Settle a reserved place: its events written there, or none to give the
    /// place up. A place already settled is ignored.
    Fill {
        place: u64,
        events: Vec<(Direction, EventBody)>,
    },
}

/// A position in a file's queue behind a reserved place.
enum Slot {
    Reserved(u64),
    Ready(Vec<(Direction, EventBody)>),
}

/// One trace file's state, kept for the run (see the module docs on `Tap`).
#[derive(Default)]
struct FileState {
    /// Named on the first write, so a file is never created empty.
    path: Option<PathBuf>,
    next_seq: u64,
    /// Records waiting behind a reserved place; empty when none is open.
    waiting: VecDeque<Slot>,
}

/// The writer task: sequences each record's events per file (the schema's
/// strictly-increasing rule holds by construction), appends them as adjacent
/// JSON lines in one write, and flushes before accepting the next record —
/// everything written before a kill is durable. Records waiting behind a
/// reserved place are held in memory until it is settled.
///
/// Each record opens its file for append and closes it again, so a long-lived
/// server holds no descriptor per session it has ever seen; only each file's
/// next `seq` is kept. That was an open handle per session until 0.6.0, which a
/// server run for days against many clients would have exhausted.
pub(super) async fn write_loop(dir: PathBuf, mut receiver: tokio::sync::mpsc::Receiver<Command>) {
    let mut writer = Writer {
        dir,
        named: 0,
        files: HashMap::new(),
        places: HashMap::new(),
    };
    while let Some(command) = receiver.recv().await {
        writer.handle(command).await;
    }
}

struct Writer {
    dir: PathBuf,
    /// Files named so far: the next file's ordinal, in first-write order.
    named: u64,
    files: HashMap<u64, FileState>,
    /// Each open reserved place and the file it is in.
    places: HashMap<u64, Arc<SessionFile>>,
}

impl Writer {
    async fn handle(&mut self, command: Command) {
        match command {
            Command::Write(Record { file, events }) => {
                let state = self.files.entry(file.id).or_default();
                if state.waiting.is_empty() {
                    append(&self.dir, &mut self.named, &file, state, events).await;
                } else {
                    state.waiting.push_back(Slot::Ready(events));
                }
            }
            Command::Reserve { file, place } => {
                let state = self.files.entry(file.id).or_default();
                state.waiting.push_back(Slot::Reserved(place));
                self.places.insert(place, file);
            }
            Command::Fill { place, events } => {
                let Some(file) = self.places.remove(&place) else {
                    return;
                };
                let state = self.files.entry(file.id).or_default();
                if let Some(slot) = state
                    .waiting
                    .iter_mut()
                    .find(|slot| matches!(slot, Slot::Reserved(reserved) if *reserved == place))
                {
                    *slot = Slot::Ready(events);
                }
                // Everything up to the next still-open place is now in order.
                while matches!(state.waiting.front(), Some(Slot::Ready(_))) {
                    if let Some(Slot::Ready(events)) = state.waiting.pop_front() {
                        append(&self.dir, &mut self.named, &file, state, events).await;
                    }
                }
            }
        }
    }
}

/// Appends `events` to `file` as adjacent lines in one write, sequenced from
/// the file's next `seq`.
async fn append(
    dir: &std::path::Path,
    named: &mut u64,
    file: &SessionFile,
    state: &mut FileState,
    events: Vec<(Direction, EventBody)>,
) {
    let first = state.next_seq;
    let mut lines = Vec::new();
    let mut seq = first;
    for (direction, body) in events {
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
        return;
    }
    // The ordinal keeps directory listings in creation order and the name
    // unique; the label makes the session ↔ trace correspondence
    // self-describing.
    let path = state.path.get_or_insert_with(|| {
        *named += 1;
        dir.join(format!("{named:03}-{}.jsonl", file.label))
    });
    let write = async {
        let mut handle = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        handle.write_all(&lines).await?;
        handle.flush().await
    };
    match write.await {
        // The seq is spent only once its line is written, so a failed write
        // leaves no gap for the next record to straddle.
        Ok(()) => state.next_seq = seq,
        Err(error) => eprintln!(
            "mcp-everything-server: tap write to {} failed: {error}",
            path.display()
        ),
    }
}
