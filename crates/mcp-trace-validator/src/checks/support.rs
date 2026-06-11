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
