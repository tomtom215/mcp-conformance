// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a release binary is, read from its own headers: the format, the
//! machine it runs on, and the one property each platform's wheel tag rests
//! on — that a Linux binary links nothing dynamically, and the oldest macOS a
//! Mach-O binary declares it runs on.
//!
//! The wheel tags are derived from these facts rather than written down beside
//! the target list, so a release build that changed linkage (a glibc target in
//! place of static musl) or its macOS deployment target fails here instead of
//! shipping a wheel whose tag promises what the binary does not do.

/// The facts about one binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Binary {
    /// 64-bit little-endian ELF.
    Elf {
        /// `e_machine`: 62 is x86-64, 183 is `AArch64`.
        machine: u16,
        /// No program interpreter and no `DT_NEEDED` entry: nothing is loaded
        /// at run time, so no C library version is required.
        is_static: bool,
    },
    /// A thin 64-bit Mach-O executable.
    MachO {
        /// `cputype`: `0x0100_0007` is x86-64, `0x0100_000c` is arm64.
        cputype: u32,
        /// The minimum macOS version, as `(major, minor)`.
        min_os: (u32, u32),
    },
    /// A PE executable.
    Pe {
        /// The COFF machine: `0x8664` is x86-64.
        machine: u16,
    },
}

pub(crate) const EM_X86_64: u16 = 62;
pub(crate) const EM_AARCH64: u16 = 183;
pub(crate) const CPU_X86_64: u32 = 0x0100_0007;
pub(crate) const CPU_ARM64: u32 = 0x0100_000c;
pub(crate) const PE_AMD64: u16 = 0x8664;

/// Reads `bytes` as one of the three formats, or says why it cannot.
pub(crate) fn inspect(bytes: &[u8]) -> Result<Binary, String> {
    match bytes.get(..4) {
        Some([0x7f, b'E', b'L', b'F']) => elf(bytes),
        Some([0xcf, 0xfa, 0xed, 0xfe]) => macho(bytes),
        Some([0xca, 0xfe, 0xba, 0xbe]) => {
            Err("a universal (fat) Mach-O; the release builds one binary per architecture".into())
        }
        Some([b'M', b'Z', ..]) => pe(bytes),
        _ => Err("not an ELF, Mach-O or PE executable".to_owned()),
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| format!("truncated header (reading offset {at})"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("truncated header (reading offset {at})"))
}

fn u64_at(bytes: &[u8], at: usize) -> Result<usize, String> {
    let low = u64::from(u32_at(bytes, at)?);
    let high = u64::from(u32_at(bytes, at + 4)?);
    usize::try_from((high << 32) | low).map_err(|_| format!("offset at {at} does not fit"))
}

fn elf(bytes: &[u8]) -> Result<Binary, String> {
    if bytes.get(4..6) != Some(&[2, 1]) {
        return Err("an ELF that is not 64-bit little-endian".to_owned());
    }
    let machine = u16_at(bytes, 18)?;
    let phoff = u64_at(bytes, 32)?;
    let phentsize = usize::from(u16_at(bytes, 54)?);
    let phnum = usize::from(u16_at(bytes, 56)?);
    let mut interpreter = false;
    let mut needed = false;
    for index in 0..phnum {
        let header = phoff + index * phentsize;
        match u32_at(bytes, header)? {
            3 => interpreter = true, // PT_INTERP
            2 => {
                // PT_DYNAMIC: 16-byte (tag, value) pairs; DT_NEEDED is 1,
                // DT_NULL (0) ends the table.
                let start = u64_at(bytes, header + 8)?;
                let size = u64_at(bytes, header + 32)?;
                let mut at = start;
                while at + 16 <= start + size {
                    match u64_at(bytes, at)? {
                        0 => break,
                        1 => needed = true,
                        _ => {}
                    }
                    at += 16;
                }
            }
            _ => {}
        }
    }
    Ok(Binary::Elf {
        machine,
        is_static: !interpreter && !needed,
    })
}

fn macho(bytes: &[u8]) -> Result<Binary, String> {
    let cputype = u32_at(bytes, 4)?;
    let ncmds = u32_at(bytes, 16)?;
    let mut at = 32; // the 64-bit header's size
    for _ in 0..ncmds {
        let command = u32_at(bytes, at)?;
        let size = u32_at(bytes, at + 4)? as usize;
        let version = match command {
            0x32 => Some(u32_at(bytes, at + 12)?), // LC_BUILD_VERSION: minos
            0x24 => Some(u32_at(bytes, at + 8)?),  // LC_VERSION_MIN_MACOSX: version
            _ => None,
        };
        if let Some(version) = version {
            // Encoded xxxx.yy.zz in nibbles: major in the high half.
            return Ok(Binary::MachO {
                cputype,
                min_os: (version >> 16, (version >> 8) & 0xff),
            });
        }
        if size == 0 {
            break;
        }
        at += size;
    }
    Err("a Mach-O with no minimum macOS version (LC_BUILD_VERSION)".to_owned())
}

fn pe(bytes: &[u8]) -> Result<Binary, String> {
    let header = u32_at(bytes, 0x3c)? as usize;
    if bytes.get(header..header + 4) != Some(b"PE\0\0") {
        return Err("an MZ file without a PE header".to_owned());
    }
    Ok(Binary::Pe {
        machine: u16_at(bytes, header + 4)?,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;

    /// A minimal 64-bit ELF header with the given program headers, each
    /// `(p_type, dynamic entries)`; dynamic entries are written after them.
    pub(crate) fn elf_with(machine: u16, segments: &[(u32, &[u64])]) -> Vec<u8> {
        let mut bytes = vec![0_u8; 64];
        bytes[..6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes[32..40].copy_from_slice(&64_u64.to_le_bytes());
        bytes[54..56].copy_from_slice(&56_u16.to_le_bytes());
        let count = u16::try_from(segments.len()).unwrap();
        bytes[56..58].copy_from_slice(&count.to_le_bytes());
        let mut dynamic_at = 64 + 56 * segments.len();
        let mut tables = Vec::new();
        for (kind, entries) in segments {
            let mut header = [0_u8; 56];
            header[..4].copy_from_slice(&kind.to_le_bytes());
            header[8..16].copy_from_slice(&(dynamic_at as u64).to_le_bytes());
            let size = entries.len() * 16;
            header[32..40].copy_from_slice(&(size as u64).to_le_bytes());
            bytes.extend_from_slice(&header);
            for tag in *entries {
                tables.extend_from_slice(&tag.to_le_bytes());
                tables.extend_from_slice(&0_u64.to_le_bytes());
            }
            dynamic_at += size;
        }
        bytes.extend_from_slice(&tables);
        bytes
    }

    /// A minimal 64-bit Mach-O with one `LC_BUILD_VERSION` naming `major.minor`.
    pub(crate) fn macho_with(cputype: u32, major: u32, minor: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; 32];
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[4..8].copy_from_slice(&cputype.to_le_bytes());
        bytes[16..20].copy_from_slice(&2_u32.to_le_bytes());
        // An unrelated command first (LC_UUID, 24 bytes), so the walk is tested.
        bytes.extend_from_slice(&0x1b_u32.to_le_bytes());
        bytes.extend_from_slice(&24_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&0x32_u32.to_le_bytes());
        bytes.extend_from_slice(&24_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes()); // platform: macOS
        bytes.extend_from_slice(&((major << 16) | (minor << 8)).to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes
    }

    /// A minimal PE: the MZ stub's pointer, the signature, the machine.
    pub(crate) fn pe_with(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x40];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40_u32.to_le_bytes());
        bytes.extend_from_slice(b"PE\0\0");
        bytes.extend_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn a_static_pie_is_static_and_a_needed_library_is_not() {
        // static-pie: a PT_DYNAMIC for self-relocation, but no DT_NEEDED.
        let static_pie = elf_with(EM_X86_64, &[(1, &[]), (2, &[0x6fff_fef5, 12, 0])]);
        assert_eq!(
            inspect(&static_pie).unwrap(),
            Binary::Elf {
                machine: EM_X86_64,
                is_static: true
            }
        );
        let dynamic = elf_with(EM_AARCH64, &[(2, &[1, 0])]);
        assert_eq!(
            inspect(&dynamic).unwrap(),
            Binary::Elf {
                machine: EM_AARCH64,
                is_static: false
            }
        );
        let interpreted = elf_with(EM_X86_64, &[(3, &[])]);
        assert_eq!(
            inspect(&interpreted).unwrap(),
            Binary::Elf {
                machine: EM_X86_64,
                is_static: false
            }
        );
    }

    #[test]
    fn mach_o_reports_its_minimum_macos_and_pe_its_machine() {
        assert_eq!(
            inspect(&macho_with(CPU_ARM64, 11, 0)).unwrap(),
            Binary::MachO {
                cputype: CPU_ARM64,
                min_os: (11, 0)
            }
        );
        assert_eq!(
            inspect(&macho_with(CPU_X86_64, 10, 12)).unwrap(),
            Binary::MachO {
                cputype: CPU_X86_64,
                min_os: (10, 12)
            }
        );
        assert_eq!(
            inspect(&pe_with(PE_AMD64)).unwrap(),
            Binary::Pe { machine: PE_AMD64 }
        );
        assert!(
            inspect(&[0xca, 0xfe, 0xba, 0xbe])
                .unwrap_err()
                .contains("universal")
        );
        assert!(inspect(b"#!/bin/sh\n").is_err());
    }
}
