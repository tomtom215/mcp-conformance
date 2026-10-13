// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Binary fixtures the suite-defined tools return.
//!
//! Both payloads were generated from their format specifications (PNG: an
//! 8-bit truecolor 1×1 red pixel — signature, `IHDR`, zlib-deflated `IDAT`,
//! `IEND`, CRC-32 per chunk; WAV: the canonical 44-byte PCM header plus two
//! 16-bit silent samples) and are structurally verified by the tests below,
//! so the constants cannot drift into invalid media unnoticed.
//!
//! The icon and annotation values the second page of every listing carries
//! ([`crate::pagination`]) live here too, for the same reason: they are data
//! the server returns, fixed so the recordings of them are reproducible.

use rmcp::model::{Annotations, Icon, Role};

/// 1×1 red-pixel PNG (69 bytes), base64-encoded — the "minimal test image"
/// the `tools-call-image` scenario asks for.
pub const TINY_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

/// Minimal PCM WAV (48 bytes: canonical header + two silent samples),
/// base64-encoded — the "minimal test audio file" of `tools-call-audio`.
pub const TINY_WAV_BASE64: &str =
    "UklGRigAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YQQAAAAAAAAA";

/// The `lastModified` every annotated item carries.
///
/// A constant rather than the clock: the captures recording this surface are
/// byte-pinned, and a timestamp that moved per run would move every golden
/// with it. The date is the revision's own, so it reads as a specimen.
pub const LAST_MODIFIED: &str = "2026-07-28T00:00:00Z";

/// The icon set every iconified item carries: [`TINY_PNG_BASE64`] as a
/// `data:` URI.
///
/// A `data:` URI because an icon is something a client may fetch, and this
/// server must never be the reason a client reaches the network: the bytes
/// travel inline. It is also a scheme a client may accept — the icons section
/// names `javascript:`, `file:`, `ftp:` and `ws:` among those a client must
/// refuse, and offers `data:` alongside HTTPS.
#[must_use]
pub fn icons() -> Vec<Icon> {
    vec![
        Icon::new(format!("data:image/png;base64,{TINY_PNG_BASE64}"))
            .with_mime_type("image/png")
            .with_sizes(vec!["1x1".to_owned()]),
    ]
}

/// The annotations every annotated item carries: both audiences, a middle
/// priority and [`LAST_MODIFIED`] — each of the three optional fields set, so
/// a recording exercises the whole `Annotations` shape.
#[must_use]
pub fn annotations() -> Annotations {
    let mut annotations = Annotations::default()
        .with_audience(vec![Role::User, Role::Assistant])
        .with_priority(0.5);
    // Assigned rather than built with `with_timestamp`, which takes a `chrono`
    // value this crate would otherwise have no use for.
    annotations.last_modified = Some(LAST_MODIFIED.to_owned());
    annotations
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Decodes canonical RFC 4648 base64 — test-local so the crate itself
    /// stays free of a base64 dependency.
    fn decode_base64(input: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let values: Vec<u32> = input
            .bytes()
            .filter(|&b| b != b'=')
            .map(|c| u32::try_from(ALPHABET.iter().position(|&a| a == c).unwrap()).unwrap())
            .collect();
        let mut out = Vec::new();
        for chunk in values.chunks(4) {
            let bits = chunk.len() * 6;
            let mut acc: u32 = 0;
            for &v in chunk {
                acc = (acc << 6) | v;
            }
            acc <<= 24 - bits;
            // 4 chars → 3 bytes, 3 → 2, 2 → 1; to_be_bytes()[0] is always 0.
            out.extend_from_slice(&acc.to_be_bytes()[1..=bits / 8]);
        }
        out
    }

    #[test]
    fn decoder_round_trips_a_known_vector() {
        // RFC 4648 §10 test vector: "foobar".
        assert_eq!(decode_base64("Zm9vYmFy"), b"foobar");
        assert_eq!(decode_base64("Zm9vYmE="), b"fooba");
        assert_eq!(decode_base64("Zm9vYg=="), b"foob");
    }

    #[test]
    fn png_fixture_is_a_real_png() {
        let bytes = decode_base64(TINY_PNG_BASE64);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
        assert_eq!(&bytes[12..16], b"IHDR", "first chunk is IHDR");
        // Width and height: 1×1, big-endian u32s right after the IHDR tag.
        assert_eq!(&bytes[16..24], &[0, 0, 0, 1, 0, 0, 0, 1]);
        assert_eq!(&bytes[bytes.len() - 8..bytes.len() - 4], b"IEND");
    }

    #[test]
    fn wav_fixture_is_a_real_wav() {
        let bytes = decode_base64(TINY_WAV_BASE64);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        // RIFF size field == total length - 8.
        let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert_eq!(riff_size as usize, bytes.len() - 8);
    }

    #[test]
    fn the_icon_serializes_per_the_schema_and_inlines_a_real_png() {
        let icons = icons();
        let value = serde_json::to_value(&icons[0]).unwrap();
        // `Icon` in both revisions' schema: `src` required, `mimeType` and
        // `sizes` (an array of `WxH` strings) optional, camelCase on the wire.
        assert_eq!(value["mimeType"], "image/png");
        assert_eq!(value["sizes"], serde_json::json!(["1x1"]));
        assert!(value.get("theme").is_none(), "unset fields are omitted");
        let src = value["src"].as_str().unwrap();
        let data = src.strip_prefix("data:image/png;base64,").unwrap();
        assert_eq!(&decode_base64(data)[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn the_annotations_set_every_field_on_the_wire() {
        let value = serde_json::to_value(annotations()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "audience": ["user", "assistant"],
                "priority": 0.5,
                "lastModified": LAST_MODIFIED,
            })
        );
    }
}
