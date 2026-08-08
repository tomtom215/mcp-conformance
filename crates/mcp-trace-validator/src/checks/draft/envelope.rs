// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `2026-07-28` message-envelope checks: `resultType`, in-flight request IDs,
//! and the error-code partition.
//!
//! Every check here reads message payloads only, so none depends on the
//! stateless session model — which is why this is the area that could land
//! first. Each is *falsifiable*: it reports what the wire shows, never what an
//! implementation intended.

use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

/// JSON-RPC's implementation-defined server-error range, which `2026-07-28`
/// partitions (`basic/index#error-codes`).
const LEGACY_RANGE: core::ops::RangeInclusive<i64> = -32019..=-32000;
/// The sub-range reserved to the MCP specification itself.
const RESERVED_RANGE: core::ops::RangeInclusive<i64> = -32099..=-32020;
/// The whole JSON-RPC reserved range, inside which application-defined codes
/// do not belong.
const JSONRPC_RESERVED: core::ops::RangeInclusive<i64> = -32768..=-32000;

/// Codes this revision defines in the reserved sub-range. A code inside
/// `RESERVED_RANGE` but absent here is one the specification does not define,
/// which the clause forbids emitting.
const DEFINED_RESERVED: &[i64] = &[-32020, -32021, -32022];

/// Codes earlier revisions defined that this one withdraws. They stay reserved
/// and are never reused, so emitting one at `2026-07-28` is a violation even
/// though it was correct at `2025-11-25`.
const WITHDRAWN: &[(i64, &str)] = &[
    (
        -32002,
        "resource not found (2025-11-25 and earlier; replaced by -32602)",
    ),
    (-32042, "URL elicitation required (2025-11-25 only)"),
];

/// The standard JSON-RPC codes, which remain valid.
const STANDARD: &[i64] = &[-32700, -32600, -32601, -32602, -32603];

/// Every `(seq, code)` an error response in the trace carried, as an integer.
///
/// Non-integer codes are `base.error-code-integer`'s business (BASE-054); they
/// are skipped here rather than double-reported.
fn error_codes<'a>(context: &'a TraceContext<'_>) -> impl Iterator<Item = (u64, i64)> + 'a {
    context.messages().filter_map(|(event, _, _)| {
        let code = event
            .message_payload()?
            .get("error")?
            .get("code")?
            .as_i64()?;
        Some((event.seq, code))
    })
}

/// `BASE-048`: every result carries `resultType` (SEP-2322).
///
/// Absence is judged only on results, never on errors or notifications. The
/// backward-compatibility rule — that a client reads an absent field as
/// `"complete"` — binds the *client's interpretation* and is excluded under
/// BASE-051; it does not licence a `2026-07-28` server to omit the field.
pub(in crate::checks) fn result_type_present(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let Some(result) = payload.get("result") else {
            continue;
        };
        if result.get("resultType").is_none() {
            sink.push(
                Some(event.seq),
                "result has no `resultType`; 2026-07-28 requires it on every result".to_owned(),
            );
        } else if !result.get("resultType").is_some_and(Value::is_string) {
            sink.push(
                Some(event.seq),
                "`resultType` is present but not a string".to_owned(),
            );
        }
    }
}

/// `BASE-045`: a request ID must not collide with one still outstanding.
///
/// Deliberately *not* `base.request-id-unique`, which implements the stricter
/// `2025-11-25` rule (never reuse an ID within a session). At `2026-07-28`
/// reuse after a response is legal, so this tracks in-flight IDs and clears
/// each when its response arrives. Pointing BASE-045 at the older check would
/// report conforming traces as violations.
pub(in crate::checks) fn request_id_unique_in_flight(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Keyed by (direction-of-sender, canonical id) so a client and a server may
    // each have their own request 1 outstanding, exactly as JSON-RPC allows.
    let mut outstanding: std::collections::HashSet<(bool, String)> =
        std::collections::HashSet::new();
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let Some(id) = payload.get("id") else {
            continue;
        };
        if id.is_null() {
            continue;
        }
        let key = (
            matches!(
                event.direction,
                mcp_conformance_core::trace::Direction::ClientToServer
            ),
            id.to_string(),
        );
        if payload.get("method").is_some() {
            if !outstanding.insert(key) {
                sink.push(
                    Some(event.seq),
                    format!(
                        "request id {id} is already outstanding for this sender; \
                         2026-07-28 forbids reusing an id before its response"
                    ),
                );
            }
        } else {
            // A response clears the peer's outstanding id.
            outstanding.remove(&(!key.0, key.1));
        }
    }
}

/// `BASE-055`: the `-32000`..`-32019` legacy sub-range is closed to new use.
pub(in crate::checks) fn error_code_legacy_subrange(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, code) in error_codes(context) {
        if LEGACY_RANGE.contains(&code) {
            sink.push(
                Some(seq),
                format!(
                    "error code {code} is in the legacy sub-range (-32000..-32019), \
                     which 2026-07-28 implementations are not to use"
                ),
            );
        }
    }
}

/// `BASE-057`: the `-32020`..`-32099` sub-range is the specification's own, and
/// only codes it defines may be emitted there.
pub(in crate::checks) fn error_code_reserved_subrange(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, code) in error_codes(context) {
        if RESERVED_RANGE.contains(&code) && !DEFINED_RESERVED.contains(&code) {
            sink.push(
                Some(seq),
                format!(
                    "error code {code} is in the MCP-reserved sub-range \
                     (-32020..-32099) but is not defined by this specification"
                ),
            );
        }
    }
}

/// `BASE-058`: codes withdrawn by this revision stay reserved and unusable.
pub(in crate::checks) fn error_code_withdrawn(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, code) in error_codes(context) {
        if let Some((_, meaning)) = WITHDRAWN.iter().find(|(withdrawn, _)| *withdrawn == code) {
            sink.push(
                Some(seq),
                format!("error code {code} — {meaning} — must not be emitted at 2026-07-28"),
            );
        }
    }
}

/// `BASE-060`: application-defined codes belong outside the JSON-RPC reserved
/// range.
///
/// Reports only codes the specification leaves undefined: a standard JSON-RPC
/// code, a defined MCP code, and the two ranges the sibling checks own are all
/// accounted for elsewhere, so this fires on the remainder — an
/// application-defined code that has been placed inside `-32768`..`-32000`.
pub(in crate::checks) fn error_code_application_range(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, code) in error_codes(context) {
        let accounted_for = STANDARD.contains(&code)
            || DEFINED_RESERVED.contains(&code)
            || LEGACY_RANGE.contains(&code)
            || RESERVED_RANGE.contains(&code);
        if JSONRPC_RESERVED.contains(&code) && !accounted_for {
            sink.push(
                Some(seq),
                format!(
                    "error code {code} is application-defined but sits inside the \
                     JSON-RPC reserved range (-32768..-32000)"
                ),
            );
        }
    }
}
