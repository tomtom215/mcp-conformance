// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A serial number for an SBOM that is a function of its content.
//!
//! `cargo cyclonedx` omits `serialNumber` when `SOURCE_DATE_EPOCH` is set,
//! which is what makes its output reproducible. `CycloneDX` does not require
//! the member, but `actions/attest` does: at v4.2.2 it takes a file for a
//! `CycloneDX` SBOM only when `bomFormat`, `specVersion` and `serialNumber`
//! are all present (`src/sbom.ts`, `checkIsCycloneDX`), and refuses any other
//! with "Unsupported SBOM format". The v0.7.0 release run stopped there,
//! before anything was published: the attestation steps run only on a tag,
//! so no rehearsal had met it.
//!
//! So the serial number is derived instead of random: a UUID made from the
//! document's own hash. The same commit still gives the same bytes, and two
//! different documents still get different serial numbers.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// The `serialNumber` of a document whose content hashes to `hash` (at least
/// 32 hexadecimal digits): a version 8 UUID, the version RFC 9562 gives to
/// values a producer derives its own way, in `CycloneDX`'s `urn:uuid:` form.
pub(super) fn serial_from(hash: &str) -> Result<String, String> {
    let hex: Vec<char> = hash.chars().take(32).collect();
    if hex.len() != 32 || !hex.iter().all(char::is_ascii_hexdigit) {
        return Err(format!(
            "not a content hash to derive a serial number from: {hash:?}"
        ));
    }
    let mut hex: Vec<char> = hex.iter().map(char::to_ascii_lowercase).collect();
    // The version nibble, and the two variant bits (10xx).
    hex[12] = '8';
    hex[16] = match hex[16].to_digit(16).unwrap_or(0) & 0x3 {
        0 => '8',
        1 => '9',
        2 => 'a',
        _ => 'b',
    };
    let part = |from: usize, to: usize| hex[from..to].iter().collect::<String>();
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        part(0, 8),
        part(8, 12),
        part(12, 16),
        part(16, 20),
        part(20, 32)
    ))
}

/// Whether `serial` has the form `CycloneDX` gives a serial number:
/// `urn:uuid:` and a lowercase UUID.
pub(super) fn is_serial(serial: &str) -> bool {
    let Some(uuid) = serial.strip_prefix("urn:uuid:") else {
        return false;
    };
    uuid.len() == 36
        && uuid.chars().enumerate().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

/// `text` with `serial` as its `serialNumber`, written on a line of its own
/// after the `specVersion` line so the rest of the document keeps its bytes.
pub(super) fn insert(text: &str, serial: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + 80);
    let mut placed = false;
    for line in text.split_inclusive('\n') {
        out.push_str(line);
        if !placed && line.trim_start().starts_with("\"specVersion\"") {
            let indent = &line[..line.len() - line.trim_start().len()];
            if !line.trim_end().ends_with(',') {
                return Err("the SBOM's specVersion is its last member".to_owned());
            }
            out.push_str(indent);
            out.push_str("\"serialNumber\": \"");
            out.push_str(serial);
            out.push_str("\",\n");
            placed = true;
        }
    }
    if placed {
        Ok(out)
    } else {
        Err("the SBOM has no specVersion line to place a serialNumber after".to_owned())
    }
}

/// `git`'s hash of `text` as a blob: a content hash from a tool every run of
/// this task already needs.
fn content_hash(root: &Path, text: &str) -> Result<String, String> {
    let failed = |error: &dyn std::fmt::Display| format!("cannot hash the SBOM with git: {error}");
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| failed(&error))?;
    child
        .stdin
        .take()
        .ok_or_else(|| failed(&"no stdin"))?
        .write_all(text.as_bytes())
        .map_err(|error| failed(&error))?;
    let output = child.wait_with_output().map_err(|error| failed(&error))?;
    if !output.status.success() {
        return Err(failed(&output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `text` with a `serialNumber` derived from its content, unless it carries
/// one already.
pub(super) fn ensure(root: &Path, text: String) -> Result<String, String> {
    let has_one = serde_json::from_str::<serde_json::Value>(&text)
        .is_ok_and(|bom| bom.get("serialNumber").is_some());
    if has_one {
        return Ok(text);
    }
    let serial = serial_from(&content_hash(root, &text)?)?;
    insert(&text, &serial)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789ABCDEF0123456789abcdef01234567";

    #[test]
    fn a_serial_is_a_version_8_uuid_of_the_hash_and_nothing_else_is() {
        let serial = serial_from(HASH).unwrap();
        assert_eq!(serial, "urn:uuid:01234567-89ab-8def-8123-456789abcdef");
        assert!(is_serial(&serial));
        // Each value of the two variant bits, from the digit they replace.
        for (digit, variant) in [('0', '8'), ('5', '9'), ('a', 'a'), ('f', 'b'), ('4', '8')] {
            let hash = format!("{}{digit}{}", &HASH[..16], &HASH[17..]);
            assert_eq!(serial_from(&hash).unwrap().chars().nth(28), Some(variant));
        }
        // Another document, another serial number.
        assert_ne!(serial_from(&HASH.replace('7', "6")).unwrap(), serial);
        for bad in ["", "0123", &HASH[..31], "g123456789abcdef0123456789abcdef"] {
            assert!(serial_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_the_cyclonedx_form_is_a_serial() {
        let good = "urn:uuid:01234567-89ab-8def-8123-456789abcdef";
        assert!(is_serial(good));
        for bad in [
            "01234567-89ab-8def-8123-456789abcdef",
            "urn:uuid:01234567-89ab-8def-8123-456789abcde",
            "urn:uuid:01234567-89ab-8def-8123-456789abcdef0",
            "urn:uuid:01234567-89AB-8def-8123-456789abcdef",
            "urn:uuid:0123456789ab-8def-8123-456789abcdef-",
            "urn:uuid:01234567x89ab-8def-8123-456789abcdef",
            "urn:uuid:01234567-89ab-8deg-8123-456789abcdef",
            "",
        ] {
            assert!(!is_serial(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_serial_goes_after_spec_version_and_the_rest_keeps_its_bytes() {
        let text = "{\n  \"bomFormat\": \"CycloneDX\",\n  \"specVersion\": \"1.5\",\n  \
                    \"version\": 1,\n  \"x\": {\n    \"specVersion\": \"inner\",\n    \"y\": 1\n  }\n}\n";
        let out = insert(text, "urn:uuid:s").unwrap();
        assert_eq!(
            out,
            "{\n  \"bomFormat\": \"CycloneDX\",\n  \"specVersion\": \"1.5\",\n  \
             \"serialNumber\": \"urn:uuid:s\",\n  \"version\": 1,\n  \"x\": {\n    \
             \"specVersion\": \"inner\",\n    \"y\": 1\n  }\n}\n"
        );
        let bom: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(bom["serialNumber"], "urn:uuid:s");
        // Nowhere to put it, or nothing after it: said, not guessed.
        assert!(insert("{\n  \"bomFormat\": \"CycloneDX\"\n}\n", "s").is_err());
        assert!(insert("{\n  \"specVersion\": \"1.5\"\n}\n", "s").is_err());
    }

    #[test]
    fn a_document_gets_a_serial_from_its_content_once() {
        let root = crate::workspace_root();
        let text = "{\n  \"bomFormat\": \"CycloneDX\",\n  \"specVersion\": \"1.5\",\n  \"version\": 1\n}\n";
        let first = ensure(&root, text.to_owned()).unwrap();
        let bom: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert!(is_serial(bom["serialNumber"].as_str().unwrap()), "{first}");
        // The same bytes give the same serial number, and other bytes another.
        assert_eq!(ensure(&root, text.to_owned()).unwrap(), first);
        let other = ensure(&root, text.replace("\"version\": 1", "\"version\": 2")).unwrap();
        assert_ne!(
            serde_json::from_str::<serde_json::Value>(&other).unwrap()["serialNumber"],
            bom["serialNumber"]
        );
        // One that has a serial number keeps it.
        assert_eq!(ensure(&root, first.clone()).unwrap(), first);
    }
}
