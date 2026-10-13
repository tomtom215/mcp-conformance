// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What one live judging pass costs `mcp-conformance watch`: the whole prefix
//! re-judged, as [`Live::judge`] does after every completed exchange.
//!
//! - **`live/pass_captured_41_events`** — the largest committed captured
//!   session (`corpus/captured/sdk-typescript-2025-11-25.jsonl`), judged whole:
//!   the pass after its last exchange.
//! - **`live/pass_synthetic_10001_events`** — a synthetic conformant session
//!   of 10,001 messages (the handshake and 4,999 `tools/call` exchanges): the
//!   pass after the last exchange of a long session, the most a pass costs.
//! - **`live/session_synthetic_2001_events`** — the same shape at 2,001
//!   messages, replayed line by line with a pass after every exchange: a whole
//!   session's judging, which grows with the square of its length.
//!
//! The crate README records the numbers and what `watch` does with them. As
//! for the validator's benches, nothing here gates.

#![allow(missing_docs, clippy::expect_used)]

use core::hint::black_box;
use std::time::SystemTime;

use criterion::{Criterion, criterion_group, criterion_main};
use mcp_conformance_cli::live::Live;

/// A conformant session of `pairs` `tools/call` exchanges after the handshake,
/// as trace lines.
fn synthetic(pairs: u64) -> Vec<String> {
    let mut lines = vec![
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"bench","version":"0"}}}}"#.to_owned(),
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"bench","version":"0"}}}}"#.to_owned(),
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#.to_owned(),
    ];
    for pair in 0..pairs {
        let request = 3 + pair * 2;
        let response = request + 1;
        let id = pair + 2;
        lines.push(format!(
            r#"{{"seq":{request},"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"echo","arguments":{{"message":"event {pair}"}}}}}}}}"#
        ));
        lines.push(format!(
            r#"{{"seq":{response},"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":"event {pair}"}}]}}}}}}"#
        ));
    }
    lines
}

fn loaded(lines: &[String]) -> Live {
    let mut live = Live::new().expect("the built-in registries load");
    for line in lines {
        live.push(line.as_bytes(), SystemTime::now());
    }
    live
}

fn captured() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/captured/sdk-typescript-2025-11-25.jsonl"
    );
    std::fs::read_to_string(path)
        .expect("the captured corpus trace")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn passes(c: &mut Criterion) {
    let mut group = c.benchmark_group("live");
    let mut live = loaded(&captured());
    group.bench_function("pass_captured_41_events", |b| {
        b.iter(|| black_box(live.judge()));
    });
    let mut live = loaded(&synthetic(4_999));
    group.sample_size(20);
    group.bench_function("pass_synthetic_10001_events", |b| {
        b.iter(|| black_box(live.judge()));
    });
    let lines = synthetic(999);
    group.sample_size(10);
    group.bench_function("session_synthetic_2001_events", |b| {
        b.iter(|| {
            let mut live = Live::new().expect("the built-in registries load");
            for line in &lines {
                if live.push(line.as_bytes(), SystemTime::now()).due {
                    black_box(live.judge());
                }
            }
        });
    });
    group.finish();
}

criterion_group!(benches, passes);
criterion_main!(benches);
