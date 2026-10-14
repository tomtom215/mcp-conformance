// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The release targets and how each ecosystem names them: the archive the
//! release attaches, the npm `os`/`cpu` pair, and the wheel platform tags —
//! the last derived from the binary itself (`binfmt.rs`).

use super::binfmt::{self, Binary};

/// The binaries every release archive carries, and so every package.
pub(crate) const BINARIES: [&str; 3] = [
    "mcp-conformance",
    "mcp-trace-validator",
    "mcp-trace-capture",
];

/// The npm scope of the per-platform packages.
pub(crate) const NPM_SCOPE: &str = "@mcp-conformance";

/// One target of release.yml's `binaries` matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) triple: &'static str,
    /// Node's `process.platform`, which npm's `os` field matches.
    pub(crate) npm_os: &'static str,
    /// Node's `process.arch`, which npm's `cpu` field matches.
    pub(crate) npm_cpu: &'static str,
}

/// release.yml's `binaries` matrix, in its order. A target added there and not
/// here is refused by the generator (an archive it does not know), and one
/// listed here but missing there fails `--require-all`.
pub(crate) const TARGETS: [Target; 5] = [
    Target {
        triple: "x86_64-unknown-linux-musl",
        npm_os: "linux",
        npm_cpu: "x64",
    },
    Target {
        triple: "aarch64-unknown-linux-musl",
        npm_os: "linux",
        npm_cpu: "arm64",
    },
    Target {
        triple: "x86_64-apple-darwin",
        npm_os: "darwin",
        npm_cpu: "x64",
    },
    Target {
        triple: "aarch64-apple-darwin",
        npm_os: "darwin",
        npm_cpu: "arm64",
    },
    Target {
        triple: "x86_64-pc-windows-msvc",
        npm_os: "win32",
        npm_cpu: "x64",
    },
];

impl Target {
    pub(crate) fn is_windows(self) -> bool {
        self.npm_os == "win32"
    }

    /// `".exe"` on Windows, else empty.
    pub(crate) fn exe(self) -> &'static str {
        if self.is_windows() { ".exe" } else { "" }
    }

    /// The archive release.yml's `Archive` step writes for this target.
    pub(crate) fn archive(self, version: &str) -> String {
        let extension = if self.is_windows() { "zip" } else { "tar.gz" };
        format!("mcp-conformance-v{version}-{}.{extension}", self.triple)
    }

    /// The directory at the archive's top level.
    pub(crate) fn archive_root(self, version: &str) -> String {
        format!("mcp-conformance-v{version}-{}", self.triple)
    }

    /// `linux-x64`, …: the npm platform package's name within the scope.
    pub(crate) fn npm_key(self) -> String {
        format!("{}-{}", self.npm_os, self.npm_cpu)
    }

    pub(crate) fn npm_package(self) -> String {
        format!("{NPM_SCOPE}/{}", self.npm_key())
    }

    /// What `binfmt` must find for a binary built for this target, apart from
    /// the version and linkage facts the wheel tag is computed from.
    fn expected_machine(self, binary: Binary) -> bool {
        match (self.npm_os, self.npm_cpu, binary) {
            ("linux", "x64", Binary::Elf { machine, .. }) => machine == binfmt::EM_X86_64,
            ("linux", "arm64", Binary::Elf { machine, .. }) => machine == binfmt::EM_AARCH64,
            ("darwin", "x64", Binary::MachO { cputype, .. }) => cputype == binfmt::CPU_X86_64,
            ("darwin", "arm64", Binary::MachO { cputype, .. }) => cputype == binfmt::CPU_ARM64,
            ("win32", "x64", Binary::Pe { machine }) => machine == binfmt::PE_AMD64,
            _ => false,
        }
    }

    /// The wheel platform tags a binary with these facts may carry, or why it
    /// may carry none.
    ///
    /// - **Linux**: only a static binary is accepted, and it is tagged both
    ///   `manylinux_2_17` (with its `manylinux2014` alias) and `musllinux_1_1`.
    ///   A static binary requires no C library at all, so it meets every
    ///   manylinux and musllinux policy; 2.17 is Rust's own floor for its
    ///   glibc targets and the oldest tag pip offers on aarch64, and 1.1 the
    ///   oldest musllinux. A dynamically linked binary is refused: its tag
    ///   would rest on the glibc symbol versions it imports, which this does
    ///   not read, and the release does not build one.
    /// - **macOS**: `macosx_<min>_<arch>`, the minimum the binary declares.
    ///   From macOS 11 pip only matches `<major>_0`, so a minimum such as 11.3
    ///   rounds up to `12_0` — narrower, never wider, than the truth.
    /// - **Windows**: `win_amd64`.
    pub(crate) fn wheel_tags(self, binary: Binary) -> Result<Vec<String>, String> {
        if !self.expected_machine(binary) {
            return Err(format!("{binary:?} is not a {} binary", self.triple));
        }
        Ok(match binary {
            Binary::Elf {
                is_static: false, ..
            } => {
                return Err(format!(
                    "a dynamically linked {} binary: refusing to guess its manylinux tag",
                    self.triple
                ));
            }
            Binary::Elf { machine, .. } => {
                let arch = if machine == binfmt::EM_X86_64 {
                    "x86_64"
                } else {
                    "aarch64"
                };
                vec![
                    format!("manylinux_2_17_{arch}"),
                    format!("manylinux2014_{arch}"),
                    format!("musllinux_1_1_{arch}"),
                ]
            }
            Binary::MachO {
                cputype,
                min_os: (major, minor),
            } => {
                let arch = if cputype == binfmt::CPU_X86_64 {
                    "x86_64"
                } else {
                    "arm64"
                };
                let version = match (major, minor) {
                    (10, minor) => format!("10_{minor}"),
                    (major, 0) if major >= 11 => format!("{major}_0"),
                    (major, _) if major >= 11 => format!("{}_0", major + 1),
                    _ => return Err(format!("a minimum macOS of {major}.{minor}")),
                };
                vec![format!("macosx_{version}_{arch}")]
            }
            Binary::Pe { .. } => vec!["win_amd64".to_owned()],
        })
    }
}

/// The target whose archive is named `file`, for `version`.
pub(crate) fn target_of_archive(file: &str, version: &str) -> Option<Target> {
    TARGETS
        .into_iter()
        .find(|target| target.archive(version) == file)
}

/// The workspace version in PEP 440 form, the form a wheel's file name and
/// metadata carry. Cargo's pre-releases map onto PEP 440's three:
/// `-alpha.N` to `aN`, `-beta.N` to `bN`, `-rc.N` to `rcN`; anything else (and
/// any build metadata) is refused rather than guessed.
pub(crate) fn pep440(version: &str) -> Result<String, String> {
    let (release, pre) = version
        .split_once('-')
        .map_or((version, None), |(r, p)| (r, Some(p)));
    let parts: Vec<&str> = release.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!("`{version}` is not MAJOR.MINOR.PATCH[-PRE]"));
    }
    let Some(pre) = pre else {
        return Ok(release.to_owned());
    };
    let (kind, number) = pre.split_once('.').unwrap_or((pre, ""));
    let short = match kind {
        "alpha" => "a",
        "beta" => "b",
        "rc" => "rc",
        _ => {
            return Err(format!(
                "pre-release `{pre}` has no PEP 440 form (alpha.N, beta.N, rc.N)"
            ));
        }
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("pre-release `{pre}` needs a number (`{kind}.N`)"));
    }
    Ok(format!("{release}{short}{number}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::binfmt::tests::{elf_with, macho_with, pe_with};
    use super::super::binfmt::{CPU_ARM64, CPU_X86_64, EM_AARCH64, EM_X86_64, PE_AMD64, inspect};
    use super::*;

    fn target(triple: &str) -> Target {
        TARGETS.into_iter().find(|t| t.triple == triple).unwrap()
    }

    #[test]
    fn npm_os_and_cpu_are_nodes_names_for_each_release_target() {
        let pairs: Vec<_> = TARGETS
            .iter()
            .map(|t| (t.triple, t.npm_package()))
            .collect();
        assert_eq!(
            pairs,
            [
                (
                    "x86_64-unknown-linux-musl",
                    "@mcp-conformance/linux-x64".to_owned()
                ),
                (
                    "aarch64-unknown-linux-musl",
                    "@mcp-conformance/linux-arm64".to_owned()
                ),
                (
                    "x86_64-apple-darwin",
                    "@mcp-conformance/darwin-x64".to_owned()
                ),
                (
                    "aarch64-apple-darwin",
                    "@mcp-conformance/darwin-arm64".to_owned()
                ),
                (
                    "x86_64-pc-windows-msvc",
                    "@mcp-conformance/win32-x64".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn archives_are_named_as_release_yml_names_them() {
        let windows = target("x86_64-pc-windows-msvc");
        assert_eq!(
            windows.archive("0.6.0"),
            "mcp-conformance-v0.6.0-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(windows.exe(), ".exe");
        let linux = target("x86_64-unknown-linux-musl");
        assert_eq!(
            linux.archive("0.6.0"),
            "mcp-conformance-v0.6.0-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            target_of_archive(&linux.archive("0.6.0"), "0.6.0"),
            Some(linux)
        );
        assert_eq!(target_of_archive(&linux.archive("0.6.0"), "0.7.0"), None);
    }

    #[test]
    fn wheel_tags_follow_from_each_binary() {
        let tags = |triple: &str, bytes: &[u8]| target(triple).wheel_tags(inspect(bytes).unwrap());
        let static_x86 = elf_with(EM_X86_64, &[(2, &[0])]);
        assert_eq!(
            tags("x86_64-unknown-linux-musl", &static_x86).unwrap(),
            [
                "manylinux_2_17_x86_64",
                "manylinux2014_x86_64",
                "musllinux_1_1_x86_64"
            ]
        );
        let static_arm = elf_with(EM_AARCH64, &[]);
        assert_eq!(
            tags("aarch64-unknown-linux-musl", &static_arm).unwrap(),
            [
                "manylinux_2_17_aarch64",
                "manylinux2014_aarch64",
                "musllinux_1_1_aarch64"
            ]
        );
        let glibc = elf_with(EM_X86_64, &[(3, &[]), (2, &[1, 0])]);
        assert!(
            tags("x86_64-unknown-linux-musl", &glibc)
                .unwrap_err()
                .contains("dynamically linked")
        );
        // The wrong machine for the archive's name.
        assert!(tags("aarch64-unknown-linux-musl", &static_x86).is_err());

        let mac = |cpu, major, minor, triple: &str| tags(triple, &macho_with(cpu, major, minor));
        assert_eq!(
            mac(CPU_X86_64, 10, 12, "x86_64-apple-darwin").unwrap(),
            ["macosx_10_12_x86_64"]
        );
        assert_eq!(
            mac(CPU_ARM64, 11, 0, "aarch64-apple-darwin").unwrap(),
            ["macosx_11_0_arm64"]
        );
        assert_eq!(
            mac(CPU_ARM64, 11, 3, "aarch64-apple-darwin").unwrap(),
            ["macosx_12_0_arm64"]
        );
        assert!(mac(CPU_ARM64, 11, 0, "x86_64-apple-darwin").is_err());

        assert_eq!(
            tags("x86_64-pc-windows-msvc", &pe_with(PE_AMD64)).unwrap(),
            ["win_amd64"]
        );
        assert!(tags("x86_64-pc-windows-msvc", &pe_with(0xaa64)).is_err());
        assert!(tags("x86_64-pc-windows-msvc", &static_x86).is_err());
    }

    #[test]
    fn versions_map_to_pep_440_or_are_refused() {
        assert_eq!(pep440("0.6.0").unwrap(), "0.6.0");
        assert_eq!(pep440("0.7.0-rc.1").unwrap(), "0.7.0rc1");
        assert_eq!(pep440("1.0.0-alpha.2").unwrap(), "1.0.0a2");
        assert_eq!(pep440("1.0.0-beta.10").unwrap(), "1.0.0b10");
        for refused in [
            "0.7.0-rc",
            "0.7.0-dev.1",
            "0.7",
            "0.7.0+build",
            "x.1.0",
            "0.7.0-rc.x",
        ] {
            assert!(pep440(refused).is_err(), "{refused}");
        }
    }
}
