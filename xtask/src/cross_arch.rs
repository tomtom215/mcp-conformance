// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask cross-arch` — build the engine crates for architectures CI's
//! own hosts never cover and run their suites there, proving M1's promise of
//! "byte-identical reports across platforms" across both axes that can perturb
//! byte-level output: **endianness** and **pointer width**. Every CI host is
//! little-endian and 64-bit (`x86-64`/`aarch64` on Linux/macOS/Windows), so the
//! canonical JSON form, the JSON and `JUnit` reports, and the golden corpus had
//! only ever been pinned 64-bit little-endian.
//!
//! Targets:
//! - `s390x-unknown-linux-gnu` — 64-bit **big-endian**, run under `qemu-user`.
//!   Two tests are out of scope here: the native frame-budget proof
//!   (`deeply_nested_value_canonicalizes_on_a_small_stack`), non-portable to an
//!   emulated stack exactly as under `cfg!(miri)`; and the `cli` suite, which
//!   execs the built binary — an `s390x` child cannot run under `qemu-user`
//!   without `binfmt`. Linker and runner come from `.cargo/config.toml`.
//! - `i686-unknown-linux-gnu` — 32-bit little-endian, run **natively** through
//!   the host's 32-bit multilib runtime. Native execution needs no skips: the
//!   whole suite runs, the `cli` subprocess tests and the deep-stack proof
//!   included. The host `cc` links it with `-m32`; no config override is needed.
//!
//! A target whose cross toolchain is absent is skipped LOUDLY, not failed; the
//! scheduled CI `cross-arch` matrix installs each arch on its own runner — the
//! 32-bit `gcc-multilib` and the s390x cross-gcc hard-conflict at the dpkg level,
//! so they cannot share one — and that matrix is the enforcement of record.
//! A byte-for-byte divergence on any architecture is a real defect.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::Path;
use std::process::Command;

/// One cross target: its triple, a human note, the install hint named in a loud
/// skip, the external binaries and files its run requires beyond the rust std
/// target, and the per-leg `cargo test` argument tails (each prefixed with
/// `test --target <triple>` by the runner).
struct CrossTarget {
    triple: &'static str,
    note: &'static str,
    install_hint: &'static str,
    required_bins: &'static [&'static str],
    required_files: &'static [&'static str],
    legs: &'static [&'static [&'static str]],
}

/// The architectures exercised, each naming exactly the test targets that bear
/// on byte-identical output. `s390x` skips the two tests an emulated big-endian
/// run cannot carry; `i686` runs natively, so the whole suite (including `cli`
/// and the deep-stack proof) runs unskipped.
const TARGETS: &[CrossTarget] = &[
    CrossTarget {
        triple: "s390x-unknown-linux-gnu",
        note: "64-bit big-endian (qemu-user)",
        install_hint: "rustup target add s390x-unknown-linux-gnu; \
                       apt-get install -y gcc-s390x-linux-gnu qemu-user-static",
        required_bins: &["s390x-linux-gnu-gcc", "qemu-s390x-static"],
        required_files: &[],
        legs: &[
            &[
                "-p",
                "mcp-conformance-core",
                "--",
                "--skip",
                "deeply_nested_value_canonicalizes_on_a_small_stack",
            ],
            &[
                "-p",
                "mcp-trace-validator",
                "--lib",
                "--test",
                "golden",
                "--test",
                "readme_examples",
                "--test",
                "pathological",
            ],
        ],
    },
    CrossTarget {
        triple: "i686-unknown-linux-gnu",
        note: "32-bit little-endian (native multilib)",
        install_hint: "rustup target add i686-unknown-linux-gnu; apt-get install -y gcc-multilib",
        required_bins: &[],
        required_files: &["/usr/lib32/libc.so.6"],
        legs: &[&["-p", "mcp-conformance-core", "-p", "mcp-trace-validator"]],
    },
];

/// Runs each available target's suites. A target whose toolchain is absent is a
/// loud skip (CI installs them and is the gate of record); the gate fails on the
/// first leg that does not pass.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    let mut ran = 0_usize;
    for target in TARGETS {
        if available(&root, target) {
            if !run_target(&root, target) {
                return false;
            }
            ran += 1;
        } else {
            eprintln!(
                "xtask: cross-arch — {} SKIPPED (toolchain absent; {})",
                target.triple, target.install_hint
            );
        }
    }
    if ran == 0 {
        eprintln!(
            "xtask: cross-arch — SKIPPED (no cross toolchain present). The scheduled \
             `cross-arch` CI job installs them and runs this gate regardless."
        );
        return true;
    }
    eprintln!(
        "xtask: cross-arch — {ran} architecture(s) pass; the canonical form, the reports, \
         and the goldens are byte-identical across endianness and pointer width"
    );
    true
}

/// Runs all of `target`'s legs; `true` when every one passed.
fn run_target(root: &Path, target: &CrossTarget) -> bool {
    for leg in target.legs {
        eprintln!(
            "xtask: cross-arch — {} ({}): cargo test --target {} {}",
            target.triple,
            target.note,
            target.triple,
            leg.join(" ")
        );
        // qemu-user gives guest threads a smaller default stack than native; a
        // generous RUST_MIN_STACK keeps the harness clear of it (the one deep
        // test is skipped on the emulated target and runs natively on i686).
        let status = Command::new("cargo")
            .args(["test", "--target", target.triple])
            .args(*leg)
            .env("RUST_MIN_STACK", "33554432")
            .current_dir(root)
            .status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!(
                    "xtask: cross-arch — {} failed with {status}: byte-identical output does \
                     not hold there (or a non-portable test entered scope)",
                    target.triple
                );
                return false;
            }
            Err(error) => {
                eprintln!("xtask: cross-arch — cannot run cargo: {error}");
                return false;
            }
        }
    }
    true
}

/// True when the rust std target is installed and every required binary and file
/// for `target` is present — each absent piece would turn the run into a
/// confusing build or exec error instead of an honest skip.
fn available(root: &Path, target: &CrossTarget) -> bool {
    let std_installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .current_dir(root)
        .output()
        .is_ok_and(|out| {
            out.status.success()
                && String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .any(|line| line.trim() == target.triple)
        });
    let bins = target.required_bins.iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    });
    let files = target
        .required_files
        .iter()
        .all(|path| Path::new(path).exists());
    std_installed && bins && files
}
