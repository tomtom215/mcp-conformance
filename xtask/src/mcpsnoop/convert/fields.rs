// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The HTTP fields of an envelope, spelled as mcpsnoop records them.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{CONNECTION, Exchange, Status};

/// HTTP header name in Go's canonical form, as mcpsnoop records it.
fn canonical(name: &str) -> String {
    name.split('-')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
            })
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// The request-side fields mcpsnoop records from a POST's headers.
fn request_fields(
    headers: &BTreeMap<String, String>,
    batch: bool,
    envelope: &mut Map<String, Value>,
) {
    for (header, field) in [
        ("mcp-protocol-version", "mcp_protocol_version"),
        ("mcp-method", "mcp_method"),
        ("mcp-name", "mcp_name"),
    ] {
        if let Some(value) = headers.get(header) {
            envelope.insert(field.to_owned(), json!(value));
        }
    }
    let params: Vec<Value> = headers
        .iter()
        .filter(|(name, _)| name.starts_with("mcp-param-"))
        .map(|(name, value)| json!({ "name": canonical(name), "value": value }))
        .collect();
    if !params.is_empty() {
        envelope.insert("mcp_param_headers".to_owned(), Value::Array(params));
    }
    // mcpsnoop records no transport headers on a batch's elements.
    if !batch {
        let mut transport = Map::new();
        for (header, field) in [
            ("accept", "accept"),
            ("content-type", "content_type"),
            ("origin", "origin"),
        ] {
            if let Some(value) = headers.get(header) {
                transport.insert(field.to_owned(), json!(value));
            }
        }
        envelope.insert("transport_headers".to_owned(), Value::Object(transport));
    }
}

pub(super) fn response_fields(status: &Status, envelope: &mut Map<String, Value>) {
    envelope.insert("status".to_owned(), json!(status.0));
    let mut transport = Map::new();
    if let Some(media) = &status.1 {
        transport.insert("content_type".to_owned(), json!(media));
    }
    envelope.insert("transport_headers".to_owned(), Value::Object(transport));
}

/// A timestamp `n` milliseconds into a fixed day.
pub(super) fn timestamp(n: usize) -> String {
    let (seconds, millis) = (n / 1000, n % 1000);
    format!(
        "2026-01-01T{:02}:{:02}:{:02}.{millis:03}Z",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The HTTP fields of one message envelope; `true` when the exchange's context
/// was all there was to give.
pub(super) fn http_message(
    envelope: &mut Map<String, Value>,
    client: bool,
    batch: bool,
    known: Option<(usize, &Exchange<'_>)>,
) -> bool {
    if batch {
        envelope.insert("batch".to_owned(), json!(true));
    }
    envelope.insert("conn_id".to_owned(), json!(CONNECTION));
    let Some((index, exchange)) = known else {
        return false;
    };
    envelope.insert("exchange".to_owned(), json!(index + 1));
    if client {
        exchange.headers.is_some_and(|headers| {
            request_fields(headers, batch, envelope);
            true
        })
    } else {
        exchange.status.as_ref().is_some_and(|status| {
            response_fields(status, envelope);
            true
        })
    }
}
