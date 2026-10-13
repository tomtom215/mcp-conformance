// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `sbom --out DIR`: one `CycloneDX` SBOM per published crate, the files
//! `release.yml` attests and attaches beside each `.crate`.
//!
//! One invocation for both callers: `release.yml`'s `package` job runs it to
//! produce the release's SBOMs, and ci.yml's `package` job runs it on every PR,
//! so a change that breaks generation fails there rather than after a tag.
//!
//! `cargo cyclonedx` writes `<name>.cdx.json` beside every workspace member's
//! manifest. This task runs it once over the workspace, moves each published
//! crate's file to `DIR/<name>-<version>.cdx.json` (the name of the `.crate` it
//! describes), drops the rest (`xtask`), and checks each document says what it
//! should: `CycloneDX`, the pinned spec version, and the crate and version its
//! file name claims. A crate without its SBOM fails, so the set stays the
//! published set when a crate is added.
//!
//! Generation flags, each deliberate:
//! - `--all-features`: the SBOM lists every dependency the crate can pull in,
//!   not only the default build's;
//! - `--target all`: a `.crate` is source and builds on every platform, so its
//!   bill of materials covers every platform's dependencies;
//! - `--spec-version 1.5`: the newest the tool emits (its default is 1.3);
//! - `SOURCE_DATE_EPOCH`: the tool honours it and then omits the random serial
//!   number, so the bytes are a function of the commit. Set from the commit
//!   time when the caller has not set it.

// See `local_gates.rs`: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about items in a binary's private modules.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The `CycloneDX` spec version requested and checked.
const SPEC_VERSION: &str = "1.5";

/// A published workspace crate: name, version, and manifest directory.
struct Package {
    name: String,
    version: String,
    dir: PathBuf,
}

/// Runs the task with `args` as `cargo xtask sbom` received them.
pub(crate) fn run(args: &[String]) -> bool {
    let out = match args {
        [flag, dir] if flag == "--out" => PathBuf::from(dir),
        _ => {
            eprintln!("xtask: sbom — usage: cargo xtask sbom --out DIR");
            return false;
        }
    };
    match generate(&crate::workspace_root(), &out) {
        Ok(written) => {
            for path in &written {
                eprintln!("xtask: sbom — wrote {}", path.display());
            }
            eprintln!(
                "xtask: sbom — {} CycloneDX {SPEC_VERSION} SBOM(s), one per published crate",
                written.len()
            );
            true
        }
        Err(message) => {
            eprintln!("xtask: sbom — {message}");
            false
        }
    }
}

/// Generates, moves and checks the SBOMs; the paths written, or why not.
fn generate(root: &Path, out: &Path) -> Result<Vec<PathBuf>, String> {
    let (published, others) = packages(root)?;
    let mut command = Command::new("cargo");
    command.current_dir(root).args([
        "cyclonedx",
        "--format",
        "json",
        "--all-features",
        "--target",
        "all",
        "--spec-version",
        SPEC_VERSION,
    ]);
    if std::env::var_os("SOURCE_DATE_EPOCH").is_none() {
        command.env("SOURCE_DATE_EPOCH", commit_time(root)?);
    }
    let status = command.status().map_err(|error| {
        format!("cannot run cargo cyclonedx (cargo install cargo-cyclonedx --locked): {error}")
    })?;
    if !status.success() {
        return Err(format!("cargo cyclonedx failed with {status}"));
    }
    for stray in others {
        // Never published, so never described; removed so it is not mistaken
        // for a release file. Absent is fine: nothing to remove.
        let _ = std::fs::remove_file(stray);
    }
    std::fs::create_dir_all(out)
        .map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    let mut written = Vec::new();
    for package in &published {
        let generated = package.dir.join(format!("{}.cdx.json", package.name));
        let text = std::fs::read_to_string(&generated).map_err(|error| {
            format!(
                "no SBOM for {} at {}: {error}",
                package.name,
                generated.display()
            )
        })?;
        check(&text, &package.name, &package.version)?;
        let target = out.join(format!("{}-{}.cdx.json", package.name, package.version));
        std::fs::write(&target, text)
            .and_then(|()| std::fs::remove_file(&generated))
            .map_err(|error| {
                format!(
                    "cannot move {} to {}: {error}",
                    generated.display(),
                    target.display()
                )
            })?;
        written.push(target);
    }
    Ok(written)
}

/// The published packages, and the generated-file paths of the unpublished
/// ones, from `cargo metadata`.
fn packages(root: &Path) -> Result<(Vec<Package>, Vec<PathBuf>), String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed".to_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata output is not JSON: {error}"))?;
    let mut published = Vec::new();
    let mut others = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let (Some(name), Some(version), Some(manifest)) = (
            package["name"].as_str(),
            package["version"].as_str(),
            package["manifest_path"].as_str(),
        ) else {
            return Err(
                "cargo metadata lists a package without name, version or manifest".to_owned(),
            );
        };
        let dir = Path::new(manifest)
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        // `publish` is null when unrestricted and `[]` for `publish = false`.
        if package["publish"].is_null() {
            published.push(Package {
                name: name.to_owned(),
                version: version.to_owned(),
                dir,
            });
        } else {
            others.push(dir.join(format!("{name}.cdx.json")));
        }
    }
    if published.is_empty() {
        return Err("cargo metadata lists no published crate".to_owned());
    }
    Ok((published, others))
}

/// The HEAD commit's time, seconds since the epoch, as `git` prints it.
fn commit_time(root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git for SOURCE_DATE_EPOCH: {error}"))?;
    let time = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || time.is_empty() || !time.bytes().all(|b| b.is_ascii_digit()) {
        return Err("cannot read the HEAD commit time; set SOURCE_DATE_EPOCH".to_owned());
    }
    Ok(time)
}

/// That `text` is a `CycloneDX` document of the pinned spec version describing
/// `name` at `version`.
fn check(text: &str, name: &str, version: &str) -> Result<(), String> {
    let bom: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("{name}'s SBOM is not JSON: {error}"))?;
    let component = &bom["metadata"]["component"];
    let found = (
        bom["bomFormat"].as_str(),
        bom["specVersion"].as_str(),
        component["name"].as_str(),
        component["version"].as_str(),
    );
    if found
        == (
            Some("CycloneDX"),
            Some(SPEC_VERSION),
            Some(name),
            Some(version),
        )
    {
        Ok(())
    } else {
        Err(format!(
            "{name}'s SBOM does not describe {name} {version} as CycloneDX {SPEC_VERSION}: \
             found {found:?}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn a_document_must_describe_its_crate_in_the_pinned_format() {
        let bom = |format: &str, spec: &str, name: &str, version: &str| {
            format!(
                r#"{{"bomFormat":"{format}","specVersion":"{spec}","metadata":{{"component":{{"name":"{name}","version":"{version}"}}}}}}"#
            )
        };
        assert!(check(&bom("CycloneDX", "1.5", "a", "0.6.0"), "a", "0.6.0").is_ok());
        assert!(check(&bom("SPDX", "1.5", "a", "0.6.0"), "a", "0.6.0").is_err());
        assert!(check(&bom("CycloneDX", "1.3", "a", "0.6.0"), "a", "0.6.0").is_err());
        assert!(check(&bom("CycloneDX", "1.5", "b", "0.6.0"), "a", "0.6.0").is_err());
        assert!(check(&bom("CycloneDX", "1.5", "a", "0.5.1"), "a", "0.6.0").is_err());
        assert!(check("not json", "a", "0.6.0").is_err());
    }
}
