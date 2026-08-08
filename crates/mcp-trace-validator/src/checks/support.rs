// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared helpers for feature-area checks: declared-capability lookups and the
//! zero-dependency encoding validators (base64, RFC 3986 scheme syntax) that several
//! areas judge against.

use serde_json::Value;

use crate::context::TraceContext;

/// Whether the server declared the capability at `path` (e.g. `["tools"]` or
/// `["resources", "subscribe"]`): every segment resolves and the final value is
/// neither `false` nor `null` — the ADR-0006 reading. Returns `None` when the trace
/// has no `initialize` result to read declarations from (judgment must abstain), and
/// `Some(declared)` otherwise.
pub(super) fn server_capability(context: &TraceContext<'_>, path: &[&str]) -> Option<bool> {
    capability_in(context.server_capabilities(), path, context)
}

/// The client-side counterpart of [`server_capability`], read from the `initialize`
/// request params.
pub(super) fn client_capability(context: &TraceContext<'_>, path: &[&str]) -> Option<bool> {
    capability_in(context.client_capabilities(), path, context)
}

fn capability_in(
    capabilities: Option<&Value>,
    path: &[&str],
    context: &TraceContext<'_>,
) -> Option<bool> {
    // No initialize result at all: there is no declaration surface, so the session's
    // capability state is unknowable rather than empty.
    context.initialize().result?;
    let Some(mut current) = capabilities else {
        return Some(false);
    };
    for segment in path {
        match current.get(segment) {
            Some(next) => current = next,
            None => return Some(false),
        }
    }
    Some(!(current.is_null() || matches!(current, Value::Bool(false))))
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
        accumulator = (accumulator << 6) | sextet;
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
mod tests {
    use super::*;

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
