// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The published trace-event JSON Schema agrees with the reader.
//!
//! The schema is for producers in other languages: a Python or TypeScript
//! recorder can check its output against it without this toolkit. That is only
//! worth anything if the schema and `reader::parse_trace` accept the same
//! records, so this test runs both over every line of the corpus and over one
//! bad record per rule the schema states, and pins the single place they are
//! documented to differ.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use mcp_conformance_core::trace::{EVENT_JSON_SCHEMA, TraceEvent};
use serde_json::Value;

fn validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(EVENT_JSON_SCHEMA).unwrap();
    assert!(
        jsonschema::meta::is_valid(&schema),
        "the schema is itself valid JSON Schema"
    );
    jsonschema::draft202012::new(&schema).unwrap()
}

/// Whether the reader accepts `line` as one trace event.
fn reader_accepts(line: &str) -> bool {
    serde_json::from_str::<TraceEvent>(line).is_ok()
}

fn schema_accepts(validator: &jsonschema::Validator, line: &str) -> bool {
    serde_json::from_str::<Value>(line).is_ok_and(|value| validator.is_valid(&value))
}

fn jsonl_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            jsonl_files(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            found.push(path);
        }
    }
}

#[test]
fn every_corpus_record_is_valid_under_the_schema() {
    let validator = validator();
    let mut files = Vec::new();
    jsonl_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus"),
        &mut files,
    );
    assert!(files.len() > 100, "found {} traces", files.len());
    let mut records = 0;
    for file in &files {
        for (index, line) in std::fs::read_to_string(file).unwrap().lines().enumerate() {
            records += 1;
            let value: Value = serde_json::from_str(line).unwrap();
            let errors: Vec<String> = validator
                .iter_errors(&value)
                .map(|error| error.to_string())
                .collect();
            assert!(
                errors.is_empty() && reader_accepts(line),
                "{}:{}: {errors:?}",
                file.display(),
                index + 1
            );
        }
    }
    assert!(records > 1000, "checked {records} records");
}

/// One record breaking each rule the schema states, with the rule named.
const REJECTED: &[(&str, &str)] = &[
    (
        "seq is required",
        r#"{"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
    ),
    (
        "seq is not negative",
        r#"{"seq":-1,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
    ),
    (
        "seq is a number",
        r#"{"seq":"0","direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
    ),
    (
        "seq fits in 64 bits",
        r#"{"seq":18446744073709551616,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
    ),
    (
        "direction is required",
        r#"{"seq":0,"transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
    ),
    (
        "direction is one of two",
        r#"{"seq":0,"direction":"both","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
    ),
    (
        "transport is one of two",
        r#"{"seq":0,"direction":"client-to-server","transport":"websocket","kind":"lifecycle","event":"transport-open"}"#,
    ),
    (
        "kind is required",
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","event":"transport-open"}"#,
    ),
    (
        "kind is one of three",
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"note"}"#,
    ),
    (
        "a message has a payload",
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message"}"#,
    ),
    (
        "a lifecycle event names its moment",
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle"}"#,
    ),
    (
        "a lifecycle moment is one of three",
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-pause"}"#,
    ),
    (
        "header values are strings",
        r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","headers":{"accept":1}}"#,
    ),
    (
        "headers are an object",
        r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","headers":null}"#,
    ),
    (
        "status is a number",
        r#"{"seq":0,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":"200"}"#,
    ),
    (
        "status fits in 16 bits",
        r#"{"seq":0,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":70000}"#,
    ),
    (
        "method is a string",
        r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":1}"#,
    ),
    (
        "ts is a string",
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open","ts":1}"#,
    ),
    ("an event is an object", "[0]"),
];

#[test]
fn each_rule_the_schema_states_is_one_the_reader_enforces() {
    let validator = validator();
    for (rule, line) in REJECTED {
        assert!(
            !reader_accepts(line),
            "reader accepts, breaking {rule:?}: {line}"
        );
        assert!(
            !schema_accepts(&validator, line),
            "schema accepts, breaking {rule:?}: {line}"
        );
    }
}

/// Records both accept that a stricter reading might not: the shapes producers
/// rely on being free to write.
#[test]
fn both_accept_extensions_nulls_and_http_without_detail() {
    let validator = validator();
    for line in [
        // An unknown member is ignored by the reader, so the schema allows it.
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open","x-producer":{"v":1}}"#,
        // Optional members may be null.
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open","ts":null}"#,
        r#"{"seq":0,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":null,"method":null}"#,
        // Any JSON is a payload, including null: judging it is the validator's job.
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":null}"#,
        r#"{"seq":18446744073709551615,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{}}"#,
    ] {
        assert!(reader_accepts(line), "reader rejects: {line}");
        assert!(schema_accepts(&validator, line), "schema rejects: {line}");
    }
}

/// The one documented divergence, pinned so the schema's description stays true:
/// JSON Schema's `integer` includes `1.0`, and the reader does not.
#[test]
fn an_integer_written_with_a_fraction_is_the_documented_divergence() {
    let validator = validator();
    for line in [
        r#"{"seq":1.0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
        r#"{"seq":0,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200.0}"#,
    ] {
        assert!(!reader_accepts(line), "{line}");
        assert!(schema_accepts(&validator, line), "{line}");
    }
    assert!(EVENT_JSON_SCHEMA.contains("`1`, not `1.0`"));
}
