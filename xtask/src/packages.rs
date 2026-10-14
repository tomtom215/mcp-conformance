// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask packages --from DIR --out DIR [--require-all]`: the npm and
//! `PyPI` packages (ADR-0021 decision 5), built from the release's own binary
//! archives so that what the registries carry is the attested binary, byte for
//! byte.
//!
//! `--from` holds the archives as release.yml's `binaries` job names them and
//! the `SHA256SUMS-binaries` its `github-release` job writes over them. Every
//! archive is checked against that file before it is opened, and every file
//! that file names must be present, so a partial download fails here. Each
//! binary is then read (`binfmt.rs`) to confirm it is for the target its
//! archive names, and to derive the wheel's platform tags. `--require-all`
//! (the release) fails unless all five targets are there; without it (CI on
//! one OS) the packages are built for whichever targets are.
//!
//! Written to `--out`, which must not exist or be empty:
//!
//! - `npm/mcp-conformance/` and `npm/<os>-<cpu>/`: package directories, for
//!   `npm pack` or `npm publish`;
//! - `pypi/mcp_conformance-<version>-py3-none-<tags>.whl`.
//!
//! Every version is the workspace version (`[workspace.package].version`), in
//! PEP 440 form for the wheel. The archives are unpacked with the system's
//! `tar` (`.tar.gz`) and `unzip`, `7z` or `tar` (`.zip`, whichever runs first),
//! which check the archive's own integrity as they read it.

// See ci_permissions.rs: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about binary-crate modules.
#![allow(clippy::redundant_pub_crate)]

mod binfmt;
mod digest;
mod npm;
mod targets;
mod wheel;
mod zip;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use targets::{BINARIES, TARGETS, Target};

/// One line for both registries' descriptions.
const DESCRIPTION: &str = "Clause-by-clause Model Context Protocol conformance: record any MCP \
                           session, judge it against the specification, run the official suite";
const HOMEPAGE: &str = "https://mcp-conformance.com";
const REPOSITORY: &str = "https://github.com/tomtom215/mcp-conformance";

/// The checksum file release.yml's `github-release` job writes.
const SUMS: &str = "SHA256SUMS-binaries";

/// Runs the task; `true` on success.
pub(crate) fn run(args: &[String]) -> bool {
    match parse(args).and_then(|(from, out, all)| generate(&from, &out, all)) {
        Ok(summary) => {
            eprintln!("xtask: packages — {summary}");
            true
        }
        Err(error) => {
            eprintln!("xtask: packages — {error}");
            false
        }
    }
}

fn parse(args: &[String]) -> Result<(PathBuf, PathBuf, bool), String> {
    let (mut from, mut out, mut all) = (None, None, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from" => from = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            "--require-all" => all = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    match (from, out) {
        (Some(from), Some(out)) => Ok((from, out, all)),
        _ => Err("usage: cargo xtask packages --from DIR --out DIR [--require-all]".to_owned()),
    }
}

/// The workspace version, read as `version-sync` reads it.
fn workspace_version() -> Result<String, String> {
    let manifest = std::fs::read_to_string(crate::workspace_root().join("Cargo.toml"))
        .map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    crate::version_sync::workspace_version(&manifest)
        .ok_or_else(|| "[workspace.package] carries no version".to_owned())
}

/// `SHA256SUMS-binaries` as file name → hex digest. `sha256sum` writes
/// `<hex>  ./<name>` (or ` *` for binary mode); both are read.
fn parse_sums(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut sums = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (hex, name) = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .ok_or_else(|| format!("{SUMS}: malformed line {line:?}"))?;
        let name = name.trim_start_matches("./");
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{SUMS}: not a SHA-256 for {name}: {hex}"));
        }
        if sums
            .insert(name.to_owned(), hex.to_ascii_lowercase())
            .is_some()
        {
            return Err(format!("{SUMS}: {name} is listed twice"));
        }
    }
    Ok(sums)
}

/// The targets whose archives are in `from`, each verified against the sums.
fn verified_archives(
    from: &Path,
    version: &str,
    all: bool,
) -> Result<Vec<(Target, PathBuf)>, String> {
    let sums_text = std::fs::read_to_string(from.join(SUMS))
        .map_err(|e| format!("cannot read {}: {e}", from.join(SUMS).display()))?;
    let sums = parse_sums(&sums_text)?;
    let mut found = Vec::new();
    for (name, want) in &sums {
        let target = targets::target_of_archive(name, version).ok_or_else(|| {
            format!("{SUMS} names {name}, which is not a v{version} archive of a known target")
        })?;
        let path = from.join(name);
        let bytes =
            std::fs::read(&path).map_err(|e| format!("{name} is listed but unreadable: {e}"))?;
        let got = digest::hex(&digest::sha256(&bytes));
        if &got != want {
            return Err(format!("{name}: SHA-256 {got}, but {SUMS} says {want}"));
        }
        found.push((target, path));
    }
    found.sort_by_key(|(target, _)| TARGETS.iter().position(|t| t == target));
    if found.is_empty() {
        return Err(format!("{SUMS} lists no archive"));
    }
    if all && found.len() != TARGETS.len() {
        let missing: Vec<_> = TARGETS
            .iter()
            .filter(|t| !found.iter().any(|(f, _)| f == *t))
            .map(|t| t.triple)
            .collect();
        return Err(format!(
            "--require-all: no archive for {}",
            missing.join(", ")
        ));
    }
    Ok(found)
}

/// Unpacks `archive` into `into` with the first system tool that succeeds.
fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    use std::ffi::OsString;
    std::fs::create_dir_all(into).map_err(|e| format!("cannot create {}: {e}", into.display()))?;
    let (a, d) = (archive.as_os_str().to_owned(), into.as_os_str().to_owned());
    let os = OsString::from;
    let attempts: Vec<(&str, Vec<OsString>)> = if archive.extension().is_some_and(|e| e == "zip") {
        let mut seven = os("-o");
        seven.push(&d);
        vec![
            ("unzip", vec![os("-q"), a.clone(), os("-d"), d.clone()]),
            ("7z", vec![os("x"), os("-y"), seven, a.clone()]),
            ("tar", vec![os("-xf"), a, os("-C"), d]),
        ]
    } else {
        vec![("tar", vec![os("-xzf"), a, os("-C"), d])]
    };
    let mut tried = Vec::new();
    for (tool, args) in &attempts {
        match Command::new(tool).args(args).output() {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => tried.push(format!(
                "{tool}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(error) => tried.push(format!("{tool}: {error}")),
        }
    }
    Err(format!(
        "cannot unpack {} ({})",
        archive.display(),
        tried.join("; ")
    ))
}

/// Writes `files` under `dir`, marking executables 0755 where modes exist.
fn write_tree(dir: &Path, files: &[npm::File]) -> Result<(), String> {
    for (name, bytes, executable) in files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = if *executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        #[cfg(not(unix))]
        let _ = executable;
    }
    Ok(())
}

fn generate(from: &Path, out: &Path, all: bool) -> Result<String, String> {
    let version = workspace_version()?;
    let python_version = targets::pep440(&version)?;
    if out.exists()
        && out
            .read_dir()
            .map_or(true, |mut entries| entries.next().is_some())
    {
        return Err(format!("{} exists and is not empty", out.display()));
    }
    let archives = verified_archives(from, &version, all)?;
    let root = crate::workspace_root();
    let common = Common {
        license: std::fs::read(root.join("LICENSE")).map_err(|e| format!("LICENSE: {e}"))?,
        readme: std::fs::read_to_string(root.join("packaging/README.md"))
            .map_err(|e| format!("packaging/README.md: {e}"))?,
        version,
        python_version,
    };
    let work = out.join(".unpacked");
    let mut built = Vec::new();
    for (target, archive) in &archives {
        let unpacked = work.join(target.triple);
        unpack(archive, &unpacked)?;
        let dir = unpacked.join(target.archive_root(&common.version));
        built.push(package_target(*target, &dir, out, &common)?);
    }
    write_tree(
        &out.join("npm/mcp-conformance"),
        &npm::main_files(&common.version, &common.readme, &common.license),
    )?;
    std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    Ok(format!(
        "version {} (PEP 440 {}); mcp-conformance and {} platform package(s) in {}: {}",
        common.version,
        common.python_version,
        built.len(),
        out.display(),
        built.join(", ")
    ))
}

/// What every target's packages share.
struct Common {
    version: String,
    python_version: String,
    license: Vec<u8>,
    readme: String,
}

/// The npm platform package and the wheel for one target, from the unpacked
/// archive directory `dir`; returns its line of the summary.
fn package_target(
    target: Target,
    dir: &Path,
    out: &Path,
    common: &Common,
) -> Result<String, String> {
    let mut binaries = Vec::new();
    let mut tags: Option<Vec<String>> = None;
    for name in BINARIES {
        let file = format!("{name}{}", target.exe());
        let bytes = std::fs::read(dir.join(&file))
            .map_err(|e| format!("{}: no {file} in the archive: {e}", target.triple))?;
        let facts =
            binfmt::inspect(&bytes).map_err(|e| format!("{}/{file}: {e}", target.triple))?;
        let these = target
            .wheel_tags(facts)
            .map_err(|e| format!("{file}: {e}"))?;
        if tags.as_ref().is_some_and(|tags| *tags != these) {
            return Err(format!(
                "{}: the binaries disagree on their platform tags",
                target.triple
            ));
        }
        tags = Some(these);
        binaries.push((name, bytes));
    }
    let tags = tags.unwrap_or_default();
    write_tree(
        &out.join("npm").join(target.npm_key()),
        &npm::platform_files(target, &common.version, &common.license, &binaries),
    )?;
    let scripts: Vec<(String, Vec<u8>)> = wheel::script_names(target.exe())
        .into_iter()
        .zip(binaries.into_iter().map(|(_, bytes)| bytes))
        .collect();
    let wheel = wheel::build(
        &common.python_version,
        &tags,
        &common.readme,
        &common.license,
        &scripts,
    )?;
    // Read back what was written: the archive must hold what was meant.
    let entries = zip::read(&wheel.bytes)?;
    if entries.len() != 9 {
        return Err(format!(
            "{}: the wheel holds {} files, not 9",
            wheel.file_name,
            entries.len()
        ));
    }
    let pypi = out.join("pypi");
    std::fs::create_dir_all(&pypi).map_err(|e| format!("{}: {e}", pypi.display()))?;
    std::fs::write(pypi.join(&wheel.file_name), &wheel.bytes)
        .map_err(|e| format!("{}: {e}", wheel.file_name))?;
    Ok(format!("{} ({})", target.npm_package(), wheel.file_name))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sums_are_read_in_both_sha256sum_forms_and_malformed_lines_refused() {
        let hex = "ab".repeat(32);
        let text = format!("{hex}  ./a.tar.gz\n{hex} *b.zip\n\n");
        let sums = parse_sums(&text).unwrap();
        assert_eq!(sums.keys().collect::<Vec<_>>(), ["a.tar.gz", "b.zip"]);
        assert!(parse_sums("nothex  a").is_err());
        assert!(
            parse_sums(&format!("{hex}  a\n{hex}  ./a\n"))
                .unwrap_err()
                .contains("twice")
        );
    }

    #[test]
    fn the_packages_carry_the_workspace_version() {
        // The version every manifest is generated with is the one Cargo.toml
        // declares, and the wheel's is its PEP 440 form.
        let version = workspace_version().unwrap();
        let manifest = npm::main_manifest(&version);
        assert_eq!(manifest["version"], version.as_str());
        for target in TARGETS {
            assert_eq!(
                npm::platform_manifest(target, &version)["version"],
                version.as_str()
            );
        }
        let wheel = wheel::metadata(&targets::pep440(&version).unwrap(), "");
        assert!(wheel.contains(&format!(
            "\nVersion: {}\n",
            targets::pep440(&version).unwrap()
        )));
    }

    #[test]
    fn arguments_are_required_and_unknown_ones_refused() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(parse(&args(&["--from", "a"])).is_err());
        assert!(parse(&args(&["--from", "a", "--out", "b", "--x"])).is_err());
        let (from, out, all) =
            parse(&args(&["--out", "b", "--from", "a", "--require-all"])).unwrap();
        assert_eq!(
            (from, out, all),
            (PathBuf::from("a"), PathBuf::from("b"), true)
        );
    }
}
