// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Canonicalization throughput benchmarks (see `benches/README.md` in
//! `mcp-trace-validator` for the no-gate policy).

// Bench targets are not public API; criterion's generated `main` has no docs.
#![allow(missing_docs)]

use core::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mcp_conformance_core::canonical::to_canonical_string;
use serde_json::{Value, json};

/// A representative MCP message payload: nested objects, mixed scalars, floats.
fn tool_result_payload() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 42,
        "result": {
            "content": [
                {"type": "text", "text": "Current weather in New York: 72°F, partly cloudy"},
                {"type": "resource_link", "uri": "file:///project/src/main.rs", "name": "main.rs"}
            ],
            "structuredContent": {
                "temperature": 72.5,
                "humidity": 0.41,
                "windSpeed": 9.999_999_999_999_997e-7,
                "stationIds": [101, 102, 103],
                "flags": {"zulu": true, "alpha": false, "mike": null}
            },
            "isError": false
        }
    })
}

fn canonicalize(criterion: &mut Criterion) {
    let payload = tool_result_payload();
    let bytes = to_canonical_string(&payload).len() as u64;
    let mut group = criterion.benchmark_group("canonical");
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("tool_result_payload", |bencher| {
        bencher.iter(|| to_canonical_string(black_box(&payload)));
    });
    group.finish();
}

criterion_group!(benches, canonicalize);
criterion_main!(benches);
