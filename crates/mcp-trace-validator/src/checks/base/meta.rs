// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `_meta` key-grammar check (`BASE-019`/`BASE-020`).

use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

/// `BASE-019`/`BASE-020`: `_meta` key grammar — an optional dotted-label
/// prefix ending in `/`, then a name that begins and ends alphanumeric.
///
/// Scope: the `params._meta` and `result._meta` objects — the
/// "property/parameter" positions the clauses name on the message envelope.
/// `_meta` objects nested deeper (content items, tool definitions) share the
/// grammar but collide with user-defined data (a tool's `arguments` may
/// legitimately contain a member spelled `_meta`), so the envelope positions
/// are the sound, false-positive-free scope.
pub(in crate::checks) fn meta_key_format(context: &TraceContext<'_>, sink: &mut FindingSink) {
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
            for key in meta.keys() {
                if let Err(reason) = validate_meta_key(key) {
                    sink.push(
                        Some(event.seq),
                        format!("{envelope}._meta key {key:?} {reason}"),
                    );
                }
            }
        }
    }
}

/// Validates one `_meta` key against the `2025-11-25` grammar: an optional
/// `label(.label)*/` prefix (labels start with a letter, end with a letter or
/// digit, interior letters/digits/hyphens) and a name that, unless empty,
/// begins and ends alphanumeric with `-`/`_`/`.`/alphanumerics between.
fn validate_meta_key(key: &str) -> Result<(), String> {
    let (prefix, name) = match key.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, key),
    };
    if let Some(prefix) = prefix {
        for label in prefix.split('.') {
            let bytes = label.as_bytes();
            let shape_ok = bytes.first().is_some_and(u8::is_ascii_alphabetic)
                && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-');
            if !shape_ok {
                return Err(format!(
                    "has prefix label {label:?}; labels must start with a letter, end with \
                     a letter or digit, and contain only letters, digits, or hyphens"
                ));
            }
        }
    }
    if !name.is_empty() {
        let bytes = name.as_bytes();
        if !bytes.first().is_some_and(u8::is_ascii_alphanumeric)
            || !bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        {
            return Err(
                "has a name that does not begin and end with an alphanumeric character".to_owned(),
            );
        }
        if !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(
                "has a name with characters outside alphanumerics, hyphens, underscores, \
                 and dots"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::checks;
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};
    use crate::report::Finding;
    use mcp_conformance_core::trace::TraceEvent;

    fn run_check(check_id: &str, trace: &str) -> Vec<Finding> {
        let events: Vec<TraceEvent> = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check_id).unwrap().run(&context)
    }

    const INIT: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#;

    #[test]
    fn meta_key_grammar_table() {
        use super::validate_meta_key;
        for valid in [
            "progressToken",
            "x",
            "n.a-me_0",
            "com.example/key",
            "com.example/",
            "a/b",
            "a1-b/n",
            "io.modelcontextprotocol/x",
        ] {
            assert!(
                validate_meta_key(valid).is_ok(),
                "{valid:?} should be valid"
            );
        }
        // Each rejection's *reason* feeds `finding.detail` verbatim, so the
        // table pins which rule fired — a defect routed to the wrong rule
        // (say, a bad label reported as a bad name) is a wrong diagnostic
        // even when the verdict is right.
        for (invalid, reason) in [
            ("1bad/x", "prefix label"), // label starts with a digit
            ("bad-/x", "prefix label"), // label ends with a hyphen
            ("a..b/x", "prefix label"), // empty interior label
            ("/x", "prefix label"),     // empty prefix label
            ("a_b/x", "prefix label"),  // underscore not allowed in labels
            ("a/-x", "begin and end"),  // name starts with a hyphen
            ("a/x.", "begin and end"),  // name ends with a dot
            ("a/x y", "characters outside"), // space in name
            ("a/b/c", "characters outside"), // slash in name
        ] {
            let error = validate_meta_key(invalid)
                .expect_err(&format!("{invalid:?} should be invalid"));
            assert!(
                error.contains(reason),
                "{invalid:?} should be rejected by the {reason:?} rule, got: {error}"
            );
        }
        // Documented edge: an empty name is allowed ("Unless empty…"), and a
        // bare empty key has no prefix either.
        assert!(validate_meta_key("").is_ok());
    }

    #[test]
    fn meta_key_format_scopes_to_envelope_meta_only() {
        // params._meta violations are findings; identical spellings inside
        // user data (tool arguments) are not.
        let trace = format!(
            "{INIT}\n{}\n{}",
            r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"ping","params":{"_meta":{"1bad./t":1}}}}"#,
            r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"_meta":{"1bad./t":1}}}}}"#
        );
        let findings = run_check("base.meta-key-format", &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].detail.contains("params._meta"), "{findings:?}");
        assert_eq!(findings[0].seq, Some(1));
    }
}
