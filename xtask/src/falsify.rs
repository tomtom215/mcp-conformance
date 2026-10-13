// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask falsify-gates`: every gate shown able to go red.
//!
//! A gate that cannot fail is a comment. For each gate [`crate::gates`] runs,
//! and for the golden corpus, this runs the gate on the clean tree (it must
//! pass, or the experiment means nothing), makes one small edit of the kind the
//! gate exists to catch, runs it again — it must fail, and must say why in the
//! words the falsifier expects, so a gate that fails for an unrelated reason
//! does not count — and restores the file byte for byte.
//!
//! The edits are made in the working tree, because the gates locate the
//! repository from where xtask was compiled. So the task refuses to start
//! unless `git status` is clean, and restores each file before the next gate
//! runs, on failure as on success. A run killed mid-gate can leave one edit
//! behind; `git status` names it and `git checkout -- <file>` undoes it.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspace_root;

/// How a falsifier changes the tree.
enum Edit {
    /// Replace the first occurrence of `from` (which must be present) with `to`.
    Replace(&'static str, &'static str),
    /// Append this many comment lines.
    AppendLines(&'static str, usize),
    /// Create a file that must not exist yet; it is removed afterwards.
    Create(&'static str),
}

/// How the gate under test is run.
#[derive(Clone, Copy)]
enum Runner {
    /// This xtask binary, as the given task. Gates that read the tree at run
    /// time need nothing rebuilt.
    Xtask,
    /// Through `cargo`, which rebuilds what embeds the edited file.
    Cargo(&'static [&'static str]),
}

/// One gate, one edit it must catch, and what it must say when it does.
struct Falsifier {
    gate: &'static str,
    runner: Runner,
    file: &'static str,
    edit: Edit,
    expect: &'static str,
}

const FALSIFIERS: &[Falsifier] = &[
    Falsifier {
        gate: "file-sizes",
        runner: Runner::Xtask,
        file: "xtask/src/version_sync.rs",
        edit: Edit::AppendLines("// falsify-gates: padding past the cap", 500),
        expect: "xtask/src/version_sync.rs",
    },
    Falsifier {
        gate: "spdx",
        runner: Runner::Xtask,
        file: "clippy.toml",
        edit: Edit::Replace("# SPDX-License-Identifier: MIT", "# Licence: MIT"),
        expect: "clippy.toml",
    },
    Falsifier {
        gate: "license-files",
        runner: Runner::Xtask,
        file: "crates/mcp-conformance-core/LICENSE",
        edit: Edit::AppendLines("falsify-gates", 1),
        expect: "mcp-conformance-core",
    },
    Falsifier {
        gate: "fuzz-targets",
        runner: Runner::Xtask,
        file: "fuzz/fuzz_targets/falsify_undeclared.rs",
        edit: Edit::Create("// SPDX-License-Identifier: MIT\nfn main() {}\n"),
        expect: "falsify_undeclared",
    },
    Falsifier {
        gate: "ci-permissions",
        runner: Runner::Xtask,
        file: ".github/workflows/ci.yml",
        edit: Edit::Replace(
            "permissions:\n  contents: read\n",
            "permissions:\n  contents: write\n",
        ),
        expect: "contents: write",
    },
    Falsifier {
        gate: "toolchain-pin",
        runner: Runner::Xtask,
        file: ".github/workflows/ci.yml",
        edit: Edit::Replace(
            "rust: [\"1.99.0\", \"1.88\"]",
            "rust: [\"1.99.1\", \"1.88\"]",
        ),
        expect: "1.99.1",
    },
    Falsifier {
        gate: "page-set",
        runner: Runner::Xtask,
        file: "crates/mcp-conformance-core/registry/2025-11-25/sources.json",
        edit: Edit::Replace(
            "\"architecture\": \"No RFC 2119 keyword instances on the page (verified at spec 1304c8fe).\"",
            "\"architecture\": \"\"",
        ),
        expect: "architecture",
    },
    Falsifier {
        gate: "register-currency",
        runner: Runner::Xtask,
        file: "docs/plan/01-ecosystem-context.md",
        edit: Edit::Replace("| Verified | 2026-08-24 |", "| Verified | 2026-02-30 |"),
        expect: "2026-02-30",
    },
    Falsifier {
        gate: "docs-links",
        runner: Runner::Xtask,
        file: "CONTRIBUTING.md",
        edit: Edit::AppendLines("[falsify-gates](no-such-file-falsify.md)", 1),
        expect: "no-such-file-falsify.md",
    },
    Falsifier {
        gate: "version-sync",
        runner: Runner::Xtask,
        file: "README.md",
        edit: Edit::Replace("**Status: `0.6.0`", "**Status: `0.5.9`"),
        expect: "0.5.9",
    },
    Falsifier {
        gate: "changelog-links",
        runner: Runner::Xtask,
        file: "CHANGELOG.md",
        edit: Edit::Replace(
            "[0.6.0]: https://github.com/tomtom215/mcp-conformance/releases/tag/v0.6.0\n",
            "",
        ),
        expect: "0.6.0",
    },
    Falsifier {
        gate: "coverage --check",
        runner: Runner::Xtask,
        file: "README.md",
        edit: Edit::Replace("| BASE | 59 | 28 | 31 | 0 |", "| BASE | 59 | 29 | 30 | 0 |"),
        expect: "README",
    },
    Falsifier {
        gate: "draft-coverage --check",
        runner: Runner::Xtask,
        file: "book/src/revisions.md",
        edit: Edit::Replace(
            "**125 of the 145 judgeable clauses**",
            "**124 of the 145 judgeable clauses**",
        ),
        expect: "124",
    },
    // A matrix row that no longer says what its committed report says.
    Falsifier {
        gate: "sdk-matrix --check",
        runner: Runner::Xtask,
        file: "book/src/sdk-matrix.md",
        edit: Edit::Replace(
            "| `2025-11-25` | pass | 39 | 0 | 0 | 18 |",
            "| `2025-11-25` | pass | 40 | 0 | 0 | 18 |",
        ),
        expect: "sdk-matrix.md has drifted",
    },
    // Last of the gates: the registry is compiled into xtask, so this one
    // rebuilds it, and restoring the file rebuilds it again on the next run.
    Falsifier {
        gate: "registry-continuity",
        runner: Runner::Cargo(&["xtask", "registry-continuity"]),
        file: "crates/mcp-conformance-core/registry/2026-07-28/completion.json",
        edit: Edit::Replace(
            "\"id\": \"COMP-007\",\n      \"level\": \"MUST\",\n      \"actor\": \"server\"",
            "\"id\": \"COMP-007\",\n      \"level\": \"MUST\",\n      \"actor\": \"client\"",
        ),
        expect: "COMP-007",
    },
    // The verdict gate: a golden report that no longer matches what the
    // validator says about its trace.
    Falsifier {
        gate: "golden corpus",
        runner: Runner::Cargo(&[
            "test",
            "-p",
            "mcp-trace-validator",
            "--test",
            "golden",
            "--",
            "violation_traces_fail_and_match_goldens",
        ]),
        file: "corpus/golden/base-001-request-id-boolean.json",
        edit: Edit::Replace("\"pass\": 16,", "\"pass\": 17,"),
        expect: "base-001-request-id-boolean",
    },
];

/// The file as it was, put back when dropped — on every exit from a falsifier,
/// a panic included.
struct Restore {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let restored = match &self.original {
            Some(bytes) => std::fs::write(&self.path, bytes),
            None => std::fs::remove_file(&self.path),
        };
        if let Err(error) = restored {
            eprintln!(
                "xtask: falsify-gates — could not restore {}: {error}; `git checkout` it",
                self.path.display()
            );
        }
    }
}

/// Applies `edit` to `path`, returning what restores it.
fn apply(path: &Path, edit: &Edit) -> Result<Restore, String> {
    if let Edit::Create(contents) = edit {
        if path.exists() {
            return Err(format!("{} exists; it should not", path.display()));
        }
        let restore = Restore {
            path: path.to_path_buf(),
            original: None,
        };
        std::fs::write(path, contents).map_err(|error| error.to_string())?;
        return Ok(restore);
    }
    let original = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text = String::from_utf8(original.clone())
        .map_err(|_| format!("{} is not UTF-8", path.display()))?;
    let edited = match edit {
        Edit::Replace(from, to) => {
            if !text.contains(from) {
                return Err(format!(
                    "{} no longer contains {from:?}; the falsifier is stale",
                    path.display()
                ));
            }
            text.replacen(from, to, 1)
        }
        Edit::AppendLines(line, count) => {
            let mut edited = text;
            for _ in 0..*count {
                edited.push_str(line);
                edited.push('\n');
            }
            edited
        }
        Edit::Create(_) => unreachable!("handled above"),
    };
    let restore = Restore {
        path: path.to_path_buf(),
        original: Some(original),
    };
    std::fs::write(path, edited).map_err(|error| error.to_string())?;
    Ok(restore)
}

/// Runs the gate; its success and everything it printed.
fn run_gate(falsifier: &Falsifier, root: &Path) -> Result<(bool, String), String> {
    let mut command = match falsifier.runner {
        Runner::Xtask => {
            let exe = std::env::current_exe().map_err(|error| error.to_string())?;
            let mut command = Command::new(exe);
            command.args(falsifier.gate.split(' '));
            command
        }
        Runner::Cargo(args) => {
            let mut command =
                Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
            command.args(args);
            command
        }
    };
    let output = command
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run {}: {error}", falsifier.gate))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((output.status.success(), text))
}

/// One falsifier: passes clean, fails edited for the stated reason.
fn falsify(falsifier: &Falsifier, root: &Path) -> Result<(), String> {
    let (clean, output) = run_gate(falsifier, root)?;
    if !clean {
        return Err(format!(
            "fails on the clean tree, so its red proves nothing:\n{output}"
        ));
    }
    let restore = apply(&root.join(falsifier.file), &falsifier.edit)?;
    let result = run_gate(falsifier, root);
    drop(restore);
    let (passed, output) = result?;
    if passed {
        return Err(format!(
            "stayed green with {} edited:\n{output}",
            falsifier.file
        ));
    }
    if !output.contains(falsifier.expect) {
        return Err(format!(
            "went red, but its output does not name {:?}, so it may have failed for \
             another reason:\n{output}",
            falsifier.expect
        ));
    }
    Ok(())
}

/// The files `git status` reports as changed or untracked; empty when clean.
fn dirty(root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("git status failed".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The `falsify-gates` task.
pub(crate) fn run() -> bool {
    let root = workspace_root();
    match dirty(&root) {
        Ok(changes) if changes.is_empty() => {}
        Ok(changes) => {
            eprintln!(
                "xtask: falsify-gates — the tree has changes; commit or stash them first, \
                 since this edits files in place:\n{changes}"
            );
            return false;
        }
        Err(error) => {
            eprintln!("xtask: falsify-gates — {error}");
            return false;
        }
    }
    let mut failed = 0_usize;
    for falsifier in FALSIFIERS {
        match falsify(falsifier, &root) {
            Ok(()) => eprintln!(
                "xtask: falsify-gates — {}: green on the clean tree, red with {} edited",
                falsifier.gate, falsifier.file
            ),
            Err(problem) => {
                eprintln!("xtask: falsify-gates — {}: {problem}", falsifier.gate);
                failed += 1;
            }
        }
    }
    match dirty(&root) {
        Ok(changes) if changes.is_empty() => {}
        Ok(changes) => {
            eprintln!("xtask: falsify-gates — the tree was not left clean:\n{changes}");
            return false;
        }
        Err(error) => {
            eprintln!("xtask: falsify-gates — {error}");
            return false;
        }
    }
    if failed > 0 {
        eprintln!(
            "xtask: falsify-gates — {failed} of {} gate(s) not shown able to fail",
            FALSIFIERS.len()
        );
        return false;
    }
    eprintln!(
        "xtask: falsify-gates — every one of {} gate(s) passed on the clean tree and went red \
         on its edit, and the tree is clean again",
        FALSIFIERS.len()
    );
    true
}

#[cfg(test)]
mod tests {
    use super::FALSIFIERS;

    /// A gate added to `gates()` without a falsifier fails here, so the set
    /// shown able to fail is the set that runs.
    #[test]
    fn every_offline_gate_has_a_falsifier() {
        let gates: Vec<&str> = crate::GATES.iter().map(|(name, _)| *name).collect();
        for gate in &gates {
            assert!(
                FALSIFIERS.iter().any(|falsifier| falsifier.gate == *gate),
                "gate {gate:?} has no falsifier"
            );
        }
        for falsifier in FALSIFIERS {
            assert!(
                gates.contains(&falsifier.gate) || falsifier.gate == "golden corpus",
                "falsifier for {:?} names no gate",
                falsifier.gate
            );
        }
    }
}
