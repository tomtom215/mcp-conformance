// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A fetch of a well-known document is not MCP traffic, whatever it sits
//! between: inserted anywhere in any corpus trace, it changes no clause's
//! judgement but those about the document itself.
//!
//! This is the property the trace format's `path` field rests on. Without
//! it, a recorder that keeps a metadata fetch would convict a conforming
//! server of the transport clauses a `GET` with no session id and no
//! protocol version would break.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use mcp_conformance_core::requirement::{Registry, RegistrySet};
use mcp_trace_validator::report::Report;
use mcp_trace_validator::{engine, reader};
use serde_json::{Value, json};

/// The clauses the fetch is evidence for.
const ABOUT_THE_DOCUMENT: [&str; 6] = [
    "AUTH-006", "AUTH-010", "AUTH-011", "AUTH-091", "AUTH-141", "AUTH-142",
];

const PATH: &str = "/.well-known/oauth-protected-resource";

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn traces(subdir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(corpus().join(subdir))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no traces in {subdir}");
    files
}

/// The trace's records with every `seq` spread out by ten, so that a pair
/// of records fits between any two without renumbering what a finding cites.
fn spread(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut record: Value = serde_json::from_str(line).unwrap();
            let seq = record["seq"].as_u64().unwrap();
            record["seq"] = json!(seq * 10);
            record
        })
        .collect()
}

/// `records` with a fetch and its answer inserted after the record at `at`.
fn with_fetch(records: &[Value], at: usize) -> Vec<Value> {
    let seq = records[at]["seq"].as_u64().unwrap();
    let mut out = records[..=at].to_vec();
    out.push(json!({"seq": seq + 1, "direction": "client-to-server",
        "transport": "streamable-http", "kind": "http", "method": "GET",
        "headers": {"accept": "application/json"}, "path": PATH}));
    out.push(json!({"seq": seq + 2, "direction": "server-to-client",
        "transport": "streamable-http", "kind": "http", "status": 200,
        "headers": {"content-type": "application/json"}, "path": PATH,
        "document": {"resource": "https://mcp.example.com",
            "authorization_servers": ["https://auth.example.com"]}}));
    out.extend_from_slice(&records[at + 1..]);
    out
}

fn judged(registry: &Registry, records: &[Value]) -> Report {
    let text: Vec<String> = records.iter().map(Value::to_string).collect();
    let events = reader::parse_trace(&text.join("\n"), &reader::Limits::default()).unwrap();
    engine::validate(registry, &events)
}

/// The report as JSON, without the rows the fetch is evidence for and the
/// totals those rows move.
fn apart_from_the_document(report: &Report) -> Value {
    let mut report = serde_json::to_value(report).unwrap();
    report.as_object_mut().unwrap().remove("totals");
    report["requirements"]
        .as_array_mut()
        .unwrap()
        .retain(|row| !ABOUT_THE_DOCUMENT.contains(&row["id"].as_str().unwrap()));
    report
}

#[test]
fn a_fetch_inserted_anywhere_changes_only_the_clauses_about_the_document() {
    let legacy = Registry::builtin_2025_11_25().unwrap();
    let modern = RegistrySet::builtin()
        .unwrap()
        .registry("2026-07-28".parse().unwrap())
        .unwrap();
    let mut insertions = 0_usize;
    for (subdir, registry) in [
        ("good", &legacy),
        ("violations", &legacy),
        ("captured", &legacy),
        ("draft/good", &modern),
        ("draft/violations", &modern),
        ("draft/captured", &modern),
    ] {
        for path in traces(subdir) {
            let records = spread(&fs::read_to_string(&path).unwrap());
            let before = apart_from_the_document(&judged(registry, &records));
            // After the first record, in the middle, and at the end: between
            // a request and its answer as well as between exchanges.
            let places = [0, records.len() / 2, records.len() - 1];
            for at in places {
                let after = apart_from_the_document(&judged(registry, &with_fetch(&records, at)));
                assert_eq!(
                    before,
                    after,
                    "{} with a fetch after record {at}",
                    path.display()
                );
                insertions += 1;
            }
        }
    }
    assert!(insertions > 1000, "the corpus was walked: {insertions}");
}

#[test]
fn the_same_records_without_their_path_are_judged_as_mcp_traffic() {
    // The control: what the test above shows is the `path`'s doing. A `GET`
    // that names no path is a request to the MCP endpoint, and a server that
    // answers it `200` with JSON is held to the clauses for that.
    let registry = Registry::builtin_2025_11_25().unwrap();
    let path = corpus().join("good/http-resource-metadata-served.jsonl");
    let records = spread(&fs::read_to_string(path).unwrap());
    let stripped: Vec<Value> = records
        .iter()
        .cloned()
        .map(|mut record| {
            record.as_object_mut().unwrap().remove("path");
            record
        })
        .collect();
    assert_eq!(judged(&registry, &records).totals.fail, 0);
    let unmarked = judged(&registry, &stripped);
    assert_ne!(
        apart_from_the_document(&judged(&registry, &records)),
        apart_from_the_document(&unmarked),
        "a fetch without its path must not be set apart"
    );
}
