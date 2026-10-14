// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! SHA-256 and base64, for two purposes: the Content Security Policy source
//! (`'sha256-…'`) that admits the page's own stylesheet and nothing else; and,
//! crate-wide, the skills extension's check that a `SKILL.md` read back is the
//! file its entry's manifest digests, before comparing the two's frontmatter.
//!
//! Written here rather than taken from a crate because the project adds no
//! dependency it can avoid (04-engineering-standards §Dependency policy), and
//! this is the whole of what is needed: a digest of a few kilobytes at a time,
//! checked against the FIPS 180-4 and RFC 4648 test vectors below and, end to
//! end, by a browser that refuses the stylesheet when the digest is wrong (the
//! screenshot suite fails on any console error, which is where a CSP violation
//! is reported).

/// The first 32 bits of the fractional parts of the cube roots of the first
/// 64 primes (FIPS 180-4 §4.2.2).
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// The initial hash value (FIPS 180-4 §5.3.3).
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The SHA-256 digest of `data`.
// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about an item in a private module that the crate reads
// elsewhere; this follows the rustc lint and quiets the clippy one.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;
    // The message, a 1 bit, zeros, and its length in bits as a big-endian
    // 64-bit integer, to a multiple of 64 bytes (§5.1.1).
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    let bits = (data.len() as u64).wrapping_mul(8);
    padded.extend_from_slice(&bits.to_be_bytes());
    for block in padded.as_chunks::<64>().0 {
        compress(&mut state, block);
    }
    let mut digest = [0_u8; 32];
    for (bytes, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *bytes = word.to_be_bytes();
    }
    digest
}

/// One 64-byte block into the state (§6.2.2).
// The working variables keep the standard's one-letter names (a–h), so the
// rounds can be checked against §6.2.2 line by line.
#[allow(clippy::many_single_char_names)]
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut schedule = [0_u32; 64];
    for (word, bytes) in schedule.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*bytes);
    }
    for t in 16..64 {
        let s0 = schedule[t - 15].rotate_right(7)
            ^ schedule[t - 15].rotate_right(18)
            ^ (schedule[t - 15] >> 3);
        let s1 = schedule[t - 2].rotate_right(17)
            ^ schedule[t - 2].rotate_right(19)
            ^ (schedule[t - 2] >> 10);
        schedule[t] = schedule[t - 16]
            .wrapping_add(s0)
            .wrapping_add(schedule[t - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (k, w) in K.iter().zip(schedule) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        // Ch and Maj in their equivalent forms whose every operator matters:
        // the standard's `(e & f) ^ (!e & g)` XORs disjoint bits (so `|` is
        // the same function), and its Maj has the same slack.
        let choose = g ^ (e & (f ^ g));
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(*k)
            .wrapping_add(w);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        // Where a and b agree, that bit; where they differ, c's.
        let majority = b ^ ((a ^ b) & (b ^ c));
        let t2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (word, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *word = word.wrapping_add(value);
    }
}

/// `data` in base64 with padding (RFC 4648 §4), the encoding CSP hash
/// sources use.
pub(super) fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let byte = |index: usize| chunk.get(index).copied().unwrap_or(0);
        let group = u32::from_be_bytes([0, byte(0), byte(1), byte(2)]);
        for index in 0..4_u32 {
            // A chunk of n bytes fills n + 1 sextets; the rest are padding.
            if index as usize <= chunk.len() {
                let sextet = (group >> (18 - 6 * index)) & 0x3f;
                out.push(char::from(ALPHABET[sextet as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The CSP source admitting exactly `text` as an inline element's content.
pub(super) fn csp_source(text: &str) -> String {
    format!("'sha256-{}'", base64(&sha256(text.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(digest: &[u8]) -> String {
        digest.iter().fold(String::new(), |mut text, byte| {
            use core::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        })
    }

    #[test]
    fn sha256_matches_the_fips_180_4_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Two blocks: the padding spills into a second one.
        assert_eq!(
            hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // 55 and 56 bytes: the last length that fits one block, and the first
        // that does not.
        assert_eq!(
            hex(&sha256(&[b'a'; 55])),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
        assert_eq!(
            hex(&sha256(&[b'a'; 56])),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
        assert_eq!(
            hex(&sha256(&[b'a'; 1000])),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "{input:?}");
        }
        // Every sextet value at the top of the alphabet.
        assert_eq!(base64(&[0xfb, 0xff, 0xbf]), "+/+/");
    }

    #[test]
    fn the_csp_source_is_the_quoted_base64_digest() {
        // The digest of the empty string, as browsers print it in a CSP error.
        assert_eq!(
            csp_source(""),
            "'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='"
        );
    }
}
