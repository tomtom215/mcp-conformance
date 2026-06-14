// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask minimal-versions` — resolve every direct dependency to the
//! oldest version its declared floor allows and build/test there, proving the
//! dependency policy's claim that "floors are the oldest versions we test
//! against" (`Cargo.toml`, §`[workspace.dependencies]`).
//!
//! Without this, a declared floor is only an assertion: normal builds resolve to
//! the *newest* compatible versions, so a floor set below what the tree can
//! actually resolve (or below an API the code uses) ships green and is never
//! exercised. `cargo +nightly -Z direct-minimal-versions` instead pins each
//! direct dependency to the minimum its requirement admits, which surfaces both
//! failure modes:
//!
//! 1. **A dishonest floor** — one declared below the version the graph resolves
//!    to (a transitive requirement forces higher) makes the resolution itself
//!    fail. The fix is to raise the floor to the minimum cargo names.
//! 2. **A floor below the API in use** — the resolution succeeds but the build
//!    fails, because the code calls something newer than the floor provides.
//!
//! The gate then runs the engine crates' suites at those floors: byte-identical
//! output must hold at the oldest supported `serde`/`serde_json`, not only at
//! whatever is newest today. `direct`-minimal (not full `-Z minimal-versions`)
//! is deliberate — it pins the dependencies this workspace *declares*, not the
//! third-party transitive crates whose own under-declared floors are not ours to
//! answer for.
//!
//! Nightly-only (the flag is unstable), so this is a loud skip without it and a
//! scheduled CI job is the enforcement of record. The generated minimal lockfile
//! is reverted on the way out (`LockGuard`), so it never leaks into the working
//! tree or a later gate.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Restores `Cargo.lock` to its pre-gate contents when dropped, so the minimal
/// lockfile this gate generates never escapes — on success, failure, or panic.
struct LockGuard {
    path: PathBuf,
    saved: Vec<u8>,
}

impl LockGuard {
    fn capture(root: &Path) -> std::io::Result<Self> {
        let path = root.join("Cargo.lock");
        let saved = fs::read(&path)?;
        Ok(Self { path, saved })
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if let Err(error) = fs::write(&self.path, &self.saved) {
            eprintln!(
                "xtask: minimal-versions — WARNING: could not restore {}: {error}",
                self.path.display()
            );
        }
    }
}

/// Resolves direct dependencies to their declared floors and builds/tests there.
/// A loud skip when nightly is absent (the scheduled job installs it and is the
/// gate of record); otherwise fails on the first floor that is dishonest, that
/// the workspace outgrew, or that breaks the engine suites.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    if !nightly_available(&root) {
        eprintln!(
            "xtask: minimal-versions — SKIPPED (nightly toolchain absent; \
             rustup toolchain install nightly --profile minimal). The scheduled \
             `minimal-versions` CI job installs it and runs this gate regardless."
        );
        return true;
    }

    // Reverted on drop, so a failed or interrupted run leaves Cargo.lock as it
    // found it.
    let _guard = match LockGuard::capture(&root) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("xtask: minimal-versions — cannot read Cargo.lock: {error}");
            return false;
        }
    };

    resolve_build_test(&root)
}

/// The three checks against the floor-pinned lockfile, in order. Split from `run`
/// so the `LockGuard` scope and the loud-skip path stay legible at a glance.
fn resolve_build_test(root: &Path) -> bool {
    // 1. Pin direct dependencies to their floors. This fails iff a declared floor
    //    is below the version the graph actually resolves to — a dishonest floor.
    eprintln!("xtask: minimal-versions — resolving direct dependencies to their declared floors");
    if !cargo(
        root,
        &[
            "+nightly",
            "generate-lockfile",
            "-Z",
            "direct-minimal-versions",
        ],
    ) {
        eprintln!(
            "xtask: minimal-versions — a declared floor is below the version the \
             workspace resolves to; raise it to the minimum cargo reports above"
        );
        return false;
    }

    // 2. The whole workspace must compile at those floors. An API newer than a
    //    floor only fails here, never in a normal newest-versions build.
    eprintln!("xtask: minimal-versions — building the workspace at the floors");
    if !cargo(
        root,
        &["check", "--locked", "--workspace", "--all-features"],
    ) {
        eprintln!(
            "xtask: minimal-versions — the workspace uses an API newer than a \
             declared floor; raise the floor (or lower the use)"
        );
        return false;
    }

    // 3. Byte-identical output must hold at the oldest supported serde/serde_json,
    //    not only at today's newest — so the engine suites run at the floors too.
    eprintln!("xtask: minimal-versions — running the engine suites at the floors");
    if !cargo(
        root,
        &[
            "test",
            "--locked",
            "-p",
            "mcp-conformance-core",
            "-p",
            "mcp-trace-validator",
        ],
    ) {
        eprintln!("xtask: minimal-versions — the engine suites fail at the declared floors");
        return false;
    }

    eprintln!(
        "xtask: minimal-versions — every declared floor resolves, the workspace builds, \
         and the engine suites pass at the oldest supported versions"
    );
    true
}

/// True when a nightly toolchain is callable (the `-Z` flag needs it).
fn nightly_available(root: &Path) -> bool {
    Command::new("cargo")
        .args(["+nightly", "--version"])
        .current_dir(root)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Runs `cargo` with `args` in `root`, inheriting stdio; `true` on success.
fn cargo(root: &Path, args: &[&str]) -> bool {
    Command::new("cargo")
        .args(args)
        .current_dir(root)
        .status()
        .is_ok_and(|status| status.success())
}
