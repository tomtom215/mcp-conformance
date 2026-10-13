// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A zip writer for wheels: stored entries (no compression), fixed
//! timestamps, Unix modes — so the same inputs always give the same bytes.
//!
//! Stored, not deflated, because deflate would be the one piece of this that
//! needs a dependency or a hand-written compressor, and xtask takes neither
//! for this. The cost is size: a wheel carries the three binaries at their
//! full size (about 20 MB for Linux `x86_64` at 0.6.0, against about 8 MB
//! gzipped). pip and `PyPI` accept stored entries (the format is plain zip,
//! APPNOTE 6.3.x, which allows method 0), and the npm tarballs are compressed
//! by `npm pack` regardless.

/// One file in the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The path inside the archive, `/`-separated.
    pub(crate) name: String,
    pub(crate) data: Vec<u8>,
    /// Recorded as mode 0755 rather than 0644; pip sets the executable bit
    /// from it when it installs the file.
    pub(crate) executable: bool,
}

/// 1980-01-01 00:00:00 in MS-DOS date form, the earliest a zip can state.
const DOS_DATE: u16 = (1 << 5) | 1;

/// The archive holding `entries` in order, or why it cannot be written (a
/// field past the 32-bit limits of a zip without the ZIP64 extension).
pub(crate) fn write(entries: &[Entry]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = u32_field(out.len(), "archive offset")?;
        let size = u32_field(entry.data.len(), &entry.name)?;
        let name_len = u16::try_from(entry.name.len())
            .map_err(|_| format!("zip: name too long: {}", entry.name))?;
        let crc = super::digest::crc32(&entry.data);
        let mode: u32 = if entry.executable {
            0o100_755
        } else {
            0o100_644
        };

        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        common_fields(&mut out, crc, size, name_len);
        out.extend_from_slice(&0_u16.to_le_bytes()); // extra field length
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&entry.data);

        central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        // Made by: Unix (3), spec 2.0 — so readers take the mode from the
        // high half of the external attributes.
        central.extend_from_slice(&0x0314_u16.to_le_bytes());
        common_fields(&mut central, crc, size, name_len);
        central.extend_from_slice(&[0; 2]); // extra field length
        central.extend_from_slice(&[0; 2]); // comment length
        central.extend_from_slice(&[0; 2]); // disk number
        central.extend_from_slice(&[0; 2]); // internal attributes
        central.extend_from_slice(&(mode << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(entry.name.as_bytes());
    }
    let count = u16::try_from(entries.len()).map_err(|_| "zip: too many entries".to_owned())?;
    let central_offset = u32_field(out.len(), "central directory offset")?;
    let central_size = u32_field(central.len(), "central directory size")?;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // this disk, the central directory's disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&[0; 2]); // comment length
    Ok(out)
}

/// The fields a local header and a central directory record share, from
/// "version needed" through the name length.
fn common_fields(out: &mut Vec<u8>, crc: u32, size: u32, name_len: u16) {
    out.extend_from_slice(&20_u16.to_le_bytes()); // version needed: 2.0
    out.extend_from_slice(&0_u16.to_le_bytes()); // flags
    out.extend_from_slice(&0_u16.to_le_bytes()); // method: stored
    out.extend_from_slice(&0_u16.to_le_bytes()); // time 00:00:00
    out.extend_from_slice(&DOS_DATE.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes()); // compressed
    out.extend_from_slice(&size.to_le_bytes()); // uncompressed
    out.extend_from_slice(&name_len.to_le_bytes());
}

fn u32_field(value: usize, what: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("zip: {what} exceeds 4 GiB (no ZIP64 support)"))
}

/// The entries of an archive this module wrote, read back through its central
/// directory: `(name, data, mode)`. For tests, and for the generator's own
/// check that what it wrote is what it meant to.
pub(crate) fn read(archive: &[u8]) -> Result<Vec<(String, Vec<u8>, u32)>, String> {
    let le16 = |at: usize| -> Result<usize, String> {
        archive
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
            .ok_or_else(|| "zip: truncated".to_owned())
    };
    let le32 = |at: usize| -> Result<u32, String> {
        archive
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "zip: truncated".to_owned())
    };
    let end = archive.len().checked_sub(22).ok_or("zip: no end record")?;
    if le32(end)? != 0x0605_4b50 {
        return Err("zip: no end record (or a comment, which this writer never adds)".to_owned());
    }
    let count = le16(end + 10)?;
    let mut at = le32(end + 16)? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if le32(at)? != 0x0201_4b50 || le16(at + 10)? != 0 {
            return Err("zip: not a stored central directory record".to_owned());
        }
        let crc = le32(at + 16)?;
        let size = le32(at + 20)? as usize;
        let name_len = le16(at + 28)?;
        let mode = le32(at + 38)? >> 16;
        let local = le32(at + 42)? as usize;
        let name = archive
            .get(at + 46..at + 46 + name_len)
            .ok_or("zip: truncated")?;
        let name = String::from_utf8(name.to_vec()).map_err(|_| "zip: name not UTF-8")?;
        let start = local + 30 + le16(local + 26)? + le16(local + 28)?;
        let data = archive
            .get(start..start + size)
            .ok_or("zip: truncated")?
            .to_vec();
        if super::digest::crc32(&data) != crc {
            return Err(format!("zip: CRC mismatch for {name}"));
        }
        entries.push((name, data, mode));
        at += 46 + name_len;
    }
    Ok(entries)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_with_their_modes_and_the_bytes_are_deterministic() {
        let entries = vec![
            Entry {
                name: "pkg/__init__.py".to_owned(),
                data: b"x = 1\n".to_vec(),
                executable: false,
            },
            Entry {
                name: "pkg-1.data/scripts/tool".to_owned(),
                data: vec![0x7f, b'E', 0, 1],
                executable: true,
            },
            Entry {
                name: "empty".to_owned(),
                data: Vec::new(),
                executable: false,
            },
        ];
        let archive = write(&entries).unwrap();
        assert_eq!(archive, write(&entries).unwrap(), "same input, same bytes");
        let back = read(&archive).unwrap();
        assert_eq!(back.len(), 3);
        for ((name, data, mode), entry) in back.iter().zip(&entries) {
            assert_eq!(name, &entry.name);
            assert_eq!(data, &entry.data);
            assert_eq!(
                *mode,
                if entry.executable {
                    0o100_755
                } else {
                    0o100_644
                }
            );
        }
    }

    #[test]
    fn a_corrupted_entry_is_refused() {
        let entries = [Entry {
            name: "a".to_owned(),
            data: b"abc".to_vec(),
            executable: false,
        }];
        let mut archive = write(&entries).unwrap();
        // The data follows the 30-byte local header and the one-byte name.
        archive[31] ^= 1;
        assert!(read(&archive).unwrap_err().contains("CRC mismatch"));
    }
}
