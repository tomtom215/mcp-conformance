// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! An uncompressed POSIX `ustar` archive, for exporting entries.
//!
//! Written here rather than taken from a crate: an export is a handful of
//! regular files with short ASCII names (library ids are at most 80 bytes),
//! which is the part of the format that is a fixed 512-byte header per file —
//! name, mode, size, checksum — followed by the bytes padded to 512. Every
//! `tar` reads it; the tests read it back with a parser of the same format,
//! and with the system's `tar` where there is one.

/// The block size.
const BLOCK: usize = 512;

/// An archive being written.
#[derive(Debug, Default)]
pub(crate) struct Archive {
    bytes: Vec<u8>,
}

/// Writes `value` as a zero-padded octal number filling `field` but its last
/// byte, which stays NUL.
fn octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    let digits = format!("{value:0width$o}");
    let start = digits.len().saturating_sub(width);
    field[..width].copy_from_slice(&digits.as_bytes()[start..]);
    field[width] = 0;
}

impl Archive {
    /// Adds a regular file. `name` must be ASCII and at most 100 bytes —
    /// what a library id and its suffixes are.
    pub(crate) fn add(&mut self, name: &str, contents: &[u8], mtime: u64) -> Result<(), String> {
        if !name.is_ascii() || name.is_empty() || name.len() > 100 {
            return Err(format!("{name:?} cannot be a tar member name"));
        }
        let mut header = [0_u8; BLOCK];
        header[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut header[100..108], 0o644);
        octal(&mut header[108..116], 0);
        octal(&mut header[116..124], 0);
        octal(&mut header[124..136], contents.len() as u64);
        octal(&mut header[136..148], mtime);
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        // The checksum is computed with its own field read as spaces.
        header[148..156].copy_from_slice(b"        ");
        let sum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
        let checksum = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(checksum.as_bytes());
        self.bytes.extend_from_slice(&header);
        self.bytes.extend_from_slice(contents);
        let padding = (BLOCK - contents.len() % BLOCK) % BLOCK;
        self.bytes.resize(self.bytes.len() + padding, 0);
        Ok(())
    }

    /// The archive, closed with its two zero blocks.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        self.bytes.resize(self.bytes.len() + 2 * BLOCK, 0);
        self.bytes
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// Reads an archive back: (name, contents) per member, checking every
    /// header's checksum and magic.
    pub(crate) fn read(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut members = Vec::new();
        let mut at = 0;
        while at + BLOCK <= bytes.len() {
            let header = &bytes[at..at + BLOCK];
            if header.iter().all(|byte| *byte == 0) {
                break;
            }
            assert_eq!(&header[257..263], b"ustar\0");
            let field = |range: core::ops::Range<usize>| {
                let text: String = header[range]
                    .iter()
                    .take_while(|byte| **byte != 0 && **byte != b' ')
                    .map(|byte| char::from(*byte))
                    .collect();
                u64::from_str_radix(&text, 8).unwrap()
            };
            let mut blank = header.to_vec();
            blank[148..156].copy_from_slice(b"        ");
            let sum: u64 = blank.iter().map(|byte| u64::from(*byte)).sum();
            assert_eq!(field(148..156), sum, "checksum");
            let name: String = header[..100]
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| char::from(*byte))
                .collect();
            let size = usize::try_from(field(124..136)).unwrap();
            let start = at + BLOCK;
            members.push((name, bytes[start..start + size].to_vec()));
            at = start + size.div_ceil(BLOCK) * BLOCK;
        }
        assert_eq!(bytes.len() % BLOCK, 0);
        members
    }

    #[test]
    fn members_round_trip() {
        let mut archive = Archive::default();
        archive.add("a.jsonl", b"{}\n", 1_700_000_000).unwrap();
        archive.add("empty.meta.json", b"", 0).unwrap();
        let big = vec![b'x'; BLOCK + 1];
        archive.add("big", &big, 0).unwrap();
        let bytes = archive.finish();
        let members = read(&bytes);
        assert_eq!(members[0], ("a.jsonl".to_owned(), b"{}\n".to_vec()));
        assert_eq!(members[1], ("empty.meta.json".to_owned(), Vec::new()));
        assert_eq!(members[2].1, big);
        assert_eq!(bytes.len(), BLOCK * (2 + 1 + 3 + 2));
    }

    #[test]
    fn names_tar_cannot_hold_are_refused() {
        let mut archive = Archive::default();
        assert!(archive.add("", b"", 0).is_err());
        assert!(archive.add(&"a".repeat(101), b"", 0).is_err());
        assert!(archive.add("é", b"", 0).is_err());
        assert!(archive.add(&"a".repeat(100), b"", 0).is_ok());
    }

    #[test]
    fn octal_fields_are_padded_and_terminated() {
        let mut field = [9_u8; 8];
        octal(&mut field, 0o644);
        assert_eq!(&field, b"0000644\0");
    }

    #[test]
    fn the_system_tar_reads_it_where_there_is_one() {
        let mut archive = Archive::default();
        archive.add("x.jsonl", b"line\n", 0).unwrap();
        let dir = std::env::temp_dir().join(format!("mcp-conformance-tar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("export.tar");
        std::fs::write(&file, archive.finish()).unwrap();
        // Absent on some CI images; the parser above is the check of record.
        if let Ok(output) = std::process::Command::new("tar")
            .arg("-tf")
            .arg(&file)
            .output()
        {
            assert!(output.status.success(), "{output:?}");
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "x.jsonl");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
