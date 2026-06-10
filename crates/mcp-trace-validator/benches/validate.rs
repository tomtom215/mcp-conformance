// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Validator throughput benchmarks: events/second through the full engine
//! (registry × trace → report) and through context construction alone (message
//! classification, lifecycle stepping, response pairing). See `README.md` next to
//! this file for the no-regression-gate policy.

// Bench targets are not public API; criterion's generated `main` has no docs, and
// panicking on a malformed *synthetic* fixture is the right failure mode here.
#![allow(missing_docs, clippy::expect_used)]

use core::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mcp_conformance_core::requirement::Registry;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::context::TraceContext;
use mcp_trace_validator::{engine, reader};

/// A conformant session of `pairs` request/response exchanges after the handshake.
fn synthetic_trace(pairs: u64) -> Vec<TraceEvent> {
    let mut lines = vec![
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"bench","version":"0"}}}}"#.to_owned(),
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"bench","version":"0"}}}}"#.to_owned(),
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#.to_owned(),
    ];
    for pair in 0..pairs {
        let request_seq = 3 + pair * 2;
        let response_seq = request_seq + 1;
        let id = pair + 2;
        lines.push(format!(
            r#"{{"seq":{request_seq},"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"echo","arguments":{{"message":"event {pair}"}}}}}}}}"#
        ));
        lines.push(format!(
            r#"{{"seq":{response_seq},"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":"event {pair}"}}]}}}}}}"#
        ));
    }
    let document = lines.join("\n");
    reader::parse_trace(&document, &reader::Limits::default()).expect("synthetic trace is valid")
}

fn validator_throughput(criterion: &mut Criterion) {
    let registry = Registry::builtin_2025_11_25().expect("embedded registry loads");
    let events = synthetic_trace(500);
    let event_count = events.len() as u64;

    let mut group = criterion.benchmark_group("validator");
    group.throughput(Throughput::Elements(event_count));
    group.bench_function("validate_1003_events", |bencher| {
        bencher.iter(|| engine::validate(black_box(&registry), black_box(&events)));
    });
    group.bench_function("context_1003_events", |bencher| {
        // Context construction alone: classification, lifecycle stepping, pairing.
        bencher.iter(|| TraceContext::new(black_box(&events)));
    });
    group.finish();
}

criterion_group!(benches, validator_throughput);
criterion_main!(benches);
