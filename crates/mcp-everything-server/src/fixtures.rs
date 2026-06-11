// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Binary fixtures the suite-defined tools return.
//!
//! Both payloads were generated from their format specifications (PNG: an
//! 8-bit truecolor 1×1 red pixel — signature, `IHDR`, zlib-deflated `IDAT`,
//! `IEND`, CRC-32 per chunk; WAV: the canonical 44-byte PCM header plus two
//! 16-bit silent samples) and are structurally verified by the tests below,
//! so the constants cannot drift into invalid media unnoticed.

/// 1×1 red-pixel PNG (69 bytes), base64-encoded — the "minimal test image"
/// the `tools-call-image` scenario asks for.
pub const TINY_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

/// Minimal PCM WAV (48 bytes: canonical header + two silent samples),
/// base64-encoded — the "minimal test audio file" of `tools-call-audio`.
pub const TINY_WAV_BASE64: &str =
    "UklGRigAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YQQAAAAAAAAA";

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
}
