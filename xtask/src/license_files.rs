// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `license-files` gate: every published crate ships the licence text.
//!
//! MIT asks that its notice be "included in all copies or substantial portions"
//! of the software, and a crate on crates.io is a copy: `license = "MIT"` in the
//! manifest names the licence without including it. Cargo packages only files
//! under a crate's own directory, so each crate carries a copy of the root
//! `LICENSE` — and this gate holds every copy byte-identical to it, so a
//! licence change cannot reach one crate and not another. Every release before
//! 0.6.0 was published without one.

// See `local_gates.rs`: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about items in a binary's private modules.
#![allow(clippy::redundant_pub_crate)]

/// Checks every publishable crate under `crates/` carries the root licence.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    let Ok(licence) = std::fs::read(root.join("LICENSE")) else {
        eprintln!("xtask: license-files — cannot read the root LICENSE");
        return false;
    };
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        eprintln!("xtask: license-files — cannot list crates/");
        return false;
    };
    let mut crates: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    crates.sort();
    let mut checked = 0;
    let mut ok = true;
    for dir in crates {
        let Ok(manifest) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        if manifest
            .lines()
            .any(|line| line.trim() == "publish = false")
        {
            continue;
        }
        checked += 1;
        match std::fs::read(dir.join("LICENSE")) {
            Ok(copy) if copy == licence => {}
            Ok(_) => {
                eprintln!(
                    "xtask: license-files — {}/LICENSE differs from the root LICENSE; copy it again",
                    dir.display()
                );
                ok = false;
            }
            Err(_) => {
                eprintln!(
                    "xtask: license-files — {} has no LICENSE; `cp LICENSE {}/`",
                    dir.display(),
                    dir.display()
                );
                ok = false;
            }
        }
    }
    if ok {
        eprintln!("xtask: license-files — all {checked} published crate(s) carry the root LICENSE");
    }
    ok && checked > 0
}
