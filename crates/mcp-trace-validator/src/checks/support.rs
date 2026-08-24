// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared helpers for feature-area checks: declared-capability lookups and the
//! zero-dependency encoding validators (base64, RFC 3986 scheme syntax) that several
//! areas judge against.

use serde_json::Value;

use crate::context::TraceContext;

/// What a trace says about one capability.
///
/// A tri-state, because *"this session did not declare it"* and *"this session
/// could not have declared it"* are different facts and only the first is a
/// violation. Both were reachable in the shipped corpus: a stdio capture that
/// begins after the handshake, an `initialize` answered with an error, and —
/// most simply — `corpus/violations/life-001-first-message-not-initialize.jsonl`,
/// two events long, which answers `tools/list` without ever handshaking.
///
/// **Deliberately no `PartialEq`.** This was an `Option<bool>` whose doc said
/// "judgment must abstain" on `None`, and eight of its nine callers discarded
/// that arm — six wrote `!= Some(false)` and one `== Some(false)`, each a
/// character away from correct and each turning an unjudgeable session into a
/// green row. TOOL-001 and LIFE-009 reported *pass* on the two-event trace
/// above, and the committed golden had blessed both. Without an equality impl
/// the only way to read a `Declaration` is to name all three arms, so the
/// abstention has to be answered rather than compared away.
#[derive(Clone, Copy, Debug)]
pub(super) enum Declaration {
    /// The declaration surface resolves `path` to something that is neither
    /// `false` nor `null` — the ADR-0006 reading.
    Declared,
    /// The declaration surface is present and `path` is not on it. This is the
    /// only arm a "supported implies declared" clause may fail on.
    Withheld,
    /// There is no declaration surface at all: the trace carries no `initialize`
    /// result, so nothing in it could have declared anything. A check that
    /// reaches this must abstain — reporting a pass here states evidence the
    /// trace does not carry (ADR-0012).
    Unknowable,
}

/// What the server declared for the capability at `path` (e.g. `["tools"]` or
/// `["resources", "subscribe"]`), read from the `initialize` result.
pub(super) fn server_capability(context: &TraceContext<'_>, path: &[&str]) -> Declaration {
    capability_in(context.server_capabilities(), path, context)
}

/// The client-side counterpart of [`server_capability`], read from the `initialize`
/// request params.
pub(super) fn client_capability(context: &TraceContext<'_>, path: &[&str]) -> Declaration {
    capability_in(context.client_capabilities(), path, context)
}

fn capability_in(
    capabilities: Option<&Value>,
    path: &[&str],
    context: &TraceContext<'_>,
) -> Declaration {
    // No initialize result at all: there is no declaration surface, so the session's
    // capability state is unknowable rather than empty.
    if context.initialize().result.is_none() {
        return Declaration::Unknowable;
    }
    let Some(mut current) = capabilities else {
        return Declaration::Withheld;
    };
    for segment in path {
        match current.get(segment) {
            Some(next) => current = next,
            None => return Declaration::Withheld,
        }
    }
    if current.is_null() || matches!(current, Value::Bool(false)) {
        Declaration::Withheld
    } else {
        Declaration::Declared
    }
}

/// `true` when `text` is standard base64 (RFC 4648 §4 alphabet, `=` padding to a
/// multiple of four, padding only at the end). Validation only — nothing is decoded.
///
/// The empty string validates: it is the base64 encoding of zero bytes. The
/// image/audio/blob content checks therefore accept an empty `data`/`blob` as
/// "properly encoded" — a deliberate decision, because the rule the registry
/// quotes is about *encoding*, and flagging empty content would be a
/// content-completeness judgment the spec does not make here (and one the
/// official suite does not make, which the agreement check would surface as a
/// divergence). Empty-but-present content is thus a pass at this layer.
pub(super) fn is_base64(text: &str) -> bool {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return false;
    }
    let padding = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    if padding > 2 {
        return false;
    }
    let content = &bytes[..bytes.len() - padding];
    content
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// The bytes `text` encodes, for standard base64 (RFC 4648 §4), as UTF-8.
///
/// `None` when `text` is not valid base64 or does not decode to UTF-8. Written
/// here rather than pulled in as a dependency: the judgment surface is
/// deliberately dependency-free (only `serde`/`serde_json`), and one alphabet
/// with one padding rule is all the `2026-07-28` header sentinel needs — its
/// values are "Base64 encoding of the UTF-8 representation"
/// (`basic/transports/streamable-http#value-encoding`). Gated with its only
/// caller, since a decoder no build path reaches is dead weight.
#[cfg(feature = "draft-2026-07-28")]
pub(super) fn decode_base64(text: &str) -> Option<String> {
    if !is_base64(text) {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    // `is_base64` has already established that `=` appears only as trailing
    // padding, so stopping at the first one cannot truncate real data.
    for byte in text.bytes().take_while(|&byte| byte != b'=') {
        let sextet = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        // `+`, not `|`: the shift clears the low six bits and a sextet occupies
        // only those, so the two are numerically identical here — and `|` would
        // be an operator no test could ever distinguish from its mutations.
        accumulator = (accumulator << 6) + sextet;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push(u8::try_from((accumulator >> bits) & 0xff).ok()?);
        }
    }
    String::from_utf8(bytes).ok()
}

/// `true` when `uri` begins with an RFC 3986 §3.1 scheme followed by `:`:
/// `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`. Judges scheme syntax only — the
/// registry documents that deeper RFC 3986 validation is out of trace scope.
pub(super) fn has_rfc3986_scheme(uri: &str) -> bool {
    let Some((scheme, _)) = uri.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    use crate::checks;
    use crate::reader::{Limits, parse_trace};

    /// Every check that reads a [`Declaration`], and the traffic that evidences
    /// the support each one judges.
    const CAPABILITY_CHECKS: [&str; 7] = [
        "tools.capability-declared",
        "tools.embedded-resource-capability",
        "resources.capability-declared",
        "prompts.capability-declared",
        "logging.capability-declared",
        "completion.capability-declared",
        "lifecycle.negotiated-capabilities-only",
    ];

    /// A session exercising every capability-gated feature, optionally preceded
    /// by a handshake declaring all of them.
    fn session(handshake: bool) -> String {
        let mut lines: Vec<String> = Vec::new();
        if handshake {
            lines.push(r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#.to_owned());
            lines.push(r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{},"resources":{},"prompts":{},"logging":{},"completions":{}},"serverInfo":{"name":"s","version":"0"}}}}"#.to_owned());
        }
        for line in [
            r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#,
            r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}}"#,
            r#"{"seq":4,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"file:///a"}}}"#,
            r#"{"seq":5,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":3,"result":{"contents":[]}}}"#,
            r#"{"seq":6,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":4,"method":"prompts/get","params":{"name":"p"}}}"#,
            r#"{"seq":7,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":4,"result":{"messages":[]}}}"#,
            r#"{"seq":8,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":5,"method":"completion/complete","params":{}}}"#,
            r#"{"seq":9,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":5,"result":{"completion":{"values":[]}}}}"#,
            r#"{"seq":10,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"x"}}}"#,
            r#"{"seq":11,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"t"}}}"#,
            r#"{"seq":12,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":6,"result":{"content":[{"type":"resource","resource":{"uri":"file:///a","text":"x"}}]}}}"#,
        ] {
            lines.push(line.to_owned());
        }
        lines.join("\n")
    }

    fn subjects_and_findings(check: &str, trace: &str) -> (u32, usize) {
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        let outcome = checks::find(check).unwrap().run(&context);
        (outcome.subjects, outcome.findings.len())
    }

    /// The rule [`Declaration::Unknowable`] exists for, over every check that
    /// reads one: a session whose declarations are not in the trace earns no
    /// verdict on whether it declared them.
    ///
    /// This asserts on the *subject count*, not on findings, and that is the
    /// whole point. An abstention and a pass both have no findings, so
    /// `findings.is_empty()` — which is what each area's own tests assert —
    /// cannot tell them apart. Eight of these callers used to read the
    /// declaration as "present unless explicitly denied"; every one still
    /// produced no findings here, and every one reported a green row on a
    /// session that never carried a declaration at all.
    #[test]
    fn no_declaration_surface_means_no_verdict() {
        let trace = session(false);
        for check in CAPABILITY_CHECKS {
            let (subjects, findings) = subjects_and_findings(check, &trace);
            assert_eq!(
                subjects, 0,
                "{check} counted a subject in a session with no initialize result, \
                 so the clause it backs reports a pass it cannot support"
            );
            assert_eq!(findings, 0, "{check} judged an unjudgeable session");
        }
    }

    /// The other half, without which the abstention above could be satisfied by
    /// a check that never judges anything: the same traffic, behind a handshake,
    /// is judged.
    #[test]
    fn a_declaration_surface_is_judged() {
        let trace = session(true);
        for check in CAPABILITY_CHECKS {
            let (subjects, findings) = subjects_and_findings(check, &trace);
            assert!(subjects > 0, "{check} found nothing to judge");
            assert_eq!(
                findings, 0,
                "{check} faulted a session that declared everything it used"
            );
        }
    }

    #[test]
    fn a_present_surface_that_withholds_the_capability_is_a_violation() {
        // The arm that must stay distinguishable from the abstention: the
        // handshake is there and declares nothing.
        let trace = session(true).replace(
            r#""capabilities":{"tools":{},"resources":{},"prompts":{},"logging":{},"completions":{}}"#,
            r#""capabilities":{}"#,
        );
        for check in CAPABILITY_CHECKS {
            let (subjects, findings) = subjects_and_findings(check, &trace);
            assert!(subjects > 0, "{check} found nothing to judge");
            assert!(findings > 0, "{check} excused an undeclared capability");
        }
    }

    #[test]
    fn base64_validation_is_exact() {
        for valid in ["", "aGk=", "aGV5", "aGV5bw==", "AB+/", "QUJDRA=="] {
            assert!(is_base64(valid), "{valid:?} should validate");
        }
        for invalid in [
            "aGk",     // length not a multiple of four
            "aGk =",   // space in alphabet
            "aGk!",    // symbol outside alphabet
            "====",    // padding longer than two
            "aG=k",    // padding before the end
            "aGV5bw=", // wrong padding length for content
        ] {
            assert!(!is_base64(invalid), "{invalid:?} should not validate");
        }
    }

    #[cfg(feature = "draft-2026-07-28")]
    #[test]
    fn base64_decoding_round_trips_the_specification_examples() {
        // The encoding table in `basic/transports/streamable-http#value-encoding`,
        // verbatim: each encoded header value must decode back to its original.
        for (encoded, original) in [
            ("SGVsbG8sIOS4lueVjA==", "Hello, 世界"),
            ("IHBhZGRlZCA=", " padded "),
            ("bGluZTEKbGluZTI=", "line1\nline2"),
            ("PT9iYXNlNjQ/bGl0ZXJhbD89", "=?base64?literal?="),
        ] {
            assert_eq!(
                decode_base64(encoded).as_deref(),
                Some(original),
                "{encoded:?} should decode to {original:?}"
            );
        }
        assert_eq!(decode_base64("").as_deref(), Some(""));
    }

    #[cfg(feature = "draft-2026-07-28")]
    #[test]
    fn base64_decoding_covers_the_whole_alphabet_and_every_padding_length() {
        // `+` and `/` are the two alphabet entries a lazy table would omit.
        assert_eq!(decode_base64("fn5+").as_deref(), Some("~~~"));
        assert_eq!(decode_base64("fn4/").as_deref(), Some("~~?"));
        // Each padding length exercises a different number of emitted bytes.
        assert_eq!(decode_base64("YQ==").as_deref(), Some("a")); // 1 byte
        assert_eq!(decode_base64("YWI=").as_deref(), Some("ab")); // 2 bytes
        assert_eq!(decode_base64("YWJj").as_deref(), Some("abc")); // 3 bytes
        // Ordering matters: the bits accumulate most-significant sextet first,
        // so a transposition must not decode to the same text.
        assert_eq!(decode_base64("YmFj").as_deref(), Some("bac"));
    }

    #[cfg(feature = "draft-2026-07-28")]
    #[test]
    fn base64_decoding_refuses_what_it_cannot_represent() {
        // Not base64 at all.
        assert_eq!(decode_base64("aGk"), None);
        assert_eq!(decode_base64("aG=k"), None);
        // Valid base64 whose bytes are not UTF-8 (0xFF is never a UTF-8 lead byte).
        assert_eq!(decode_base64("/w=="), None);
    }

    #[test]
    fn rfc3986_scheme_validation_is_exact() {
        for valid in ["https://x", "file:///a", "git://r", "a:", "z+ssh.2-x:rest"] {
            assert!(has_rfc3986_scheme(valid), "{valid:?} should validate");
        }
        for invalid in [
            "",           // no scheme at all
            "no-colon",   // not a URI
            ":rest",      // empty scheme
            "1https://x", // scheme must start with ALPHA
            "ht tp://x",  // space inside scheme
            "ht_tp://x",  // underscore is not scheme syntax
        ] {
            assert!(
                !has_rfc3986_scheme(invalid),
                "{invalid:?} should not validate"
            );
        }
    }
}
