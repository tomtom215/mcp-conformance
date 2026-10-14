// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One wheel per release target, written directly (no build backend): the
//! binaries as the wheel's `scripts` data, which pip installs into the
//! environment's `bin`/`Scripts` directory unchanged and executable — the
//! release binary itself on `PATH`, not a Python wrapper around it — plus the
//! small `mcp_conformance` package (`packaging/python/`) that finds them for
//! `python -m mcp_conformance`.
//!
//! The layout and metadata follow the binary distribution format and core
//! metadata 2.4 specifications (packaging.python.org): `.dist-info/` last,
//! `RECORD` listing every other file with its SHA-256, `WHEEL` with
//! `Root-Is-Purelib: false` and one `Tag:` line per tag the file name
//! compresses, and the licence under `.dist-info/licenses/`.

use std::fmt::Write as _;

use super::digest::{base64url, sha256};
use super::targets::BINARIES;
use super::zip::{self, Entry};
use super::{DESCRIPTION, HOMEPAGE, REPOSITORY};

const INIT: &str = include_str!("../../../packaging/python/__init__.py");
const MAIN: &str = include_str!("../../../packaging/python/__main__.py");

/// The distribution name, in the normalized form file names use.
const DIST: &str = "mcp_conformance";

/// A wheel's file name and bytes.
#[derive(Debug)]
pub(crate) struct Wheel {
    pub(crate) file_name: String,
    pub(crate) bytes: Vec<u8>,
}

/// The core metadata (`METADATA`) for `version` (PEP 440 form).
pub(crate) fn metadata(version: &str, readme: &str) -> String {
    format!(
        "Metadata-Version: 2.4\n\
         Name: mcp-conformance\n\
         Version: {version}\n\
         Summary: {DESCRIPTION}\n\
         Author: Tom F.\n\
         License-Expression: MIT\n\
         License-File: LICENSE\n\
         Project-URL: Homepage, {HOMEPAGE}\n\
         Project-URL: Source, {REPOSITORY}\n\
         Project-URL: Changelog, {REPOSITORY}/blob/main/CHANGELOG.md\n\
         Keywords: mcp,model-context-protocol,conformance,testing,validator\n\
         Classifier: Environment :: Console\n\
         Classifier: Intended Audience :: Developers\n\
         Classifier: Programming Language :: Rust\n\
         Classifier: Topic :: Software Development :: Testing\n\
         Requires-Python: >=3.8\n\
         Description-Content-Type: text/markdown\n\
         \n\
         {readme}"
    )
}

/// The `WHEEL` file: one `Tag:` line per platform tag.
pub(crate) fn wheel_file(tags: &[String]) -> String {
    let mut text =
        "Wheel-Version: 1.0\nGenerator: cargo xtask packages\nRoot-Is-Purelib: false\n".to_owned();
    for tag in tags {
        let _ = writeln!(text, "Tag: py3-none-{tag}");
    }
    text
}

/// The wheel's file name: `{dist}-{version}-py3-none-{tags joined by .}.whl`.
pub(crate) fn file_name(version: &str, tags: &[String]) -> String {
    format!("{DIST}-{version}-py3-none-{}.whl", tags.join("."))
}

/// Builds the wheel. `version` is in PEP 440 form; `binaries` are
/// `(file name, bytes)` with any `.exe` already applied.
pub(crate) fn build(
    version: &str,
    tags: &[String],
    readme: &str,
    license: &[u8],
    binaries: &[(String, Vec<u8>)],
) -> Result<Wheel, String> {
    let data = format!("{DIST}-{version}.data");
    let info = format!("{DIST}-{version}.dist-info");
    let mut entries = vec![
        entry(format!("{DIST}/__init__.py"), INIT.as_bytes(), false),
        entry(format!("{DIST}/__main__.py"), MAIN.as_bytes(), false),
    ];
    for (name, bytes) in binaries {
        entries.push(entry(format!("{data}/scripts/{name}"), bytes, true));
    }
    entries.push(entry(
        format!("{info}/METADATA"),
        metadata(version, readme).as_bytes(),
        false,
    ));
    entries.push(entry(
        format!("{info}/WHEEL"),
        wheel_file(tags).as_bytes(),
        false,
    ));
    entries.push(entry(format!("{info}/licenses/LICENSE"), license, false));
    let mut record = String::new();
    for item in &entries {
        let _ = writeln!(
            record,
            "{},sha256={},{}",
            item.name,
            base64url(&sha256(&item.data)),
            item.data.len()
        );
    }
    let _ = writeln!(record, "{info}/RECORD,,");
    entries.push(entry(format!("{info}/RECORD"), record.as_bytes(), false));
    Ok(Wheel {
        file_name: file_name(version, tags),
        bytes: zip::write(&entries)?,
    })
}

fn entry(name: String, data: &[u8], executable: bool) -> Entry {
    Entry {
        name,
        data: data.to_vec(),
        executable,
    }
}

/// The binaries a wheel for a target carries, by the name pip will install.
pub(crate) fn script_names(exe: &str) -> Vec<String> {
    BINARIES.iter().map(|name| format!("{name}{exe}")).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tags() -> Vec<String> {
        [
            "manylinux_2_17_x86_64",
            "manylinux2014_x86_64",
            "musllinux_1_1_x86_64",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn the_file_name_and_wheel_file_carry_every_tag() {
        assert_eq!(
            file_name("0.6.0", &tags()),
            "mcp_conformance-0.6.0-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.musllinux_1_1_x86_64.whl"
        );
        let wheel = wheel_file(&tags());
        assert!(wheel.contains("Root-Is-Purelib: false\n"));
        assert!(wheel.contains("Tag: py3-none-musllinux_1_1_x86_64\n"));
        assert_eq!(wheel.matches("Tag: ").count(), 3);
    }

    #[test]
    fn metadata_names_the_version_and_licence() {
        let text = metadata("0.7.0rc1", "# readme\n");
        assert!(
            text.starts_with("Metadata-Version: 2.4\nName: mcp-conformance\nVersion: 0.7.0rc1\n")
        );
        assert!(text.contains("License-Expression: MIT\n"));
        assert!(
            text.ends_with("\n\n# readme\n"),
            "the body follows one blank line"
        );
    }

    #[test]
    fn record_lists_every_file_with_its_digest_and_scripts_are_executable() {
        let binaries: Vec<(String, Vec<u8>)> = script_names("")
            .into_iter()
            .map(|name| (name, vec![0x7f, b'E']))
            .collect();
        let wheel = build("0.6.0", &tags(), "readme", b"MIT", &binaries).unwrap();
        let entries = zip::read(&wheel.bytes).unwrap();
        let (last, record, _) = entries.last().unwrap();
        assert_eq!(last, "mcp_conformance-0.6.0.dist-info/RECORD");
        let record = String::from_utf8(record.clone()).unwrap();
        for (name, data, mode) in &entries[..entries.len() - 1] {
            let line = format!("{name},sha256={},{}", base64url(&sha256(data)), data.len());
            assert!(record.lines().any(|l| l == line), "{line}");
            let executable = name.contains(".data/scripts/");
            assert_eq!(*mode & 0o111 != 0, executable, "{name}");
        }
        assert!(record.ends_with("mcp_conformance-0.6.0.dist-info/RECORD,,\n"));
        assert!(
            entries
                .iter()
                .any(|(n, ..)| n == "mcp_conformance-0.6.0.data/scripts/mcp-trace-capture")
        );
        assert!(
            entries
                .iter()
                .any(|(n, ..)| n == "mcp_conformance-0.6.0.dist-info/licenses/LICENSE")
        );
    }
}
