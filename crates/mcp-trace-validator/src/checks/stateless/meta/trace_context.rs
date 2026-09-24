// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `BASE-040`: the OpenTelemetry trace-context keys, and their formats.
//!
//! Split from [`super`] because it is the one `_meta` clause that is not about
//! the protocol's own fields. `traceparent`, `tracestate` and `baggage` are the
//! specification's single exception to the `_meta` prefix rule — reserved
//! outright "to maintain compatibility with existing implementations and
//! OpenTelemetry semantic conventions for MCP" — so the grammar they must
//! follow is W3C's, not MCP's, and it is dense enough to read on its own.

use serde_json::Value;

use super::super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// `BASE-040`: `traceparent` and `tracestate`/`baggage` follow their W3C formats.
///
/// Only the `traceparent` grammar is fixed enough to judge from a trace: version
/// `00`, a 32-hex trace id that is not all zeroes, a 16-hex parent id that is not
/// all zeroes, and 2 hex flags. `tracestate` and `baggage` are list formats whose
/// members are vendor-defined, so only their gross shape is checked.
pub(in crate::checks) fn trace_context_format(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        for envelope in ["params", "result"] {
            let meta = payload
                .get(envelope)
                .and_then(|member| member.get("_meta"))
                .and_then(Value::as_object);
            let Some(meta) = meta else { continue };
            // The subject is a trace-context field that is actually present:
            // the clause binds their format, not their use.
            if let Some(value) = meta.get("traceparent") {
                sink.examined();
                if let Err(reason) = validate_traceparent(value) {
                    sink.push(
                        Some(event.seq),
                        format!("{envelope}._meta.traceparent {reason}"),
                    );
                }
            }
            for key in ["tracestate", "baggage"] {
                if let Some(value) = meta.get(key) {
                    sink.examined();
                    if !value.is_string() {
                        sink.push(
                            Some(event.seq),
                            format!("{envelope}._meta.{key} is not a string"),
                        );
                    }
                }
            }
        }
    }
}

/// The W3C Trace Context `traceparent` grammar, version `00`.
fn validate_traceparent(value: &Value) -> Result<(), String> {
    let Some(text) = value.as_str() else {
        return Err("is not a string".to_owned());
    };
    let parts: Vec<&str> = text.split('-').collect();
    let [version, trace_id, parent_id, flags] = parts.as_slice() else {
        return Err(format!(
            "is {text:?}; W3C Trace Context requires four `-`-separated fields"
        ));
    };
    let hex = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    };
    if version.len() != 2 || !hex(version) {
        return Err(format!(
            "has version {version:?}; expected two lowercase hex digits"
        ));
    }
    if trace_id.len() != 32 || !hex(trace_id) {
        return Err(format!(
            "has trace-id {trace_id:?}; expected 32 lowercase hex digits"
        ));
    }
    if trace_id.bytes().all(|b| b == b'0') {
        return Err("has an all-zero trace-id, which W3C Trace Context forbids".to_owned());
    }
    if parent_id.len() != 16 || !hex(parent_id) {
        return Err(format!(
            "has parent-id {parent_id:?}; expected 16 lowercase hex digits"
        ));
    }
    if parent_id.bytes().all(|b| b == b'0') {
        return Err("has an all-zero parent-id, which W3C Trace Context forbids".to_owned());
    }
    if flags.len() != 2 || !hex(flags) {
        return Err(format!(
            "has flags {flags:?}; expected two lowercase hex digits"
        ));
    }
    Ok(())
}
