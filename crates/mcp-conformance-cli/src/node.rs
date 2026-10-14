// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Node.js, npx, and running the pinned suite with them.
//!
//! The suite is a Node package. `test` never installs Node or anything else:
//! it checks that Node and npx are on PATH and says what to install when they
//! are not. npx fetches the suite package itself, at exactly
//! [`suite::VERSION`]: the spec is an exact version, `--yes` only answers
//! npx's "install this package?" prompt for that spec, npx runs in the run's
//! own work directory (so no `node_modules` of the caller's is consulted), and
//! the suite's `--version` is read back and compared with the pin before
//! anything is run.

use std::path::Path;
use std::process::Stdio;

use mcp_conformance_cli::suite;
use tokio::io::AsyncBufReadExt as _;
use tokio::process::Command;

/// The oldest Node.js major version the suite's dependencies support.
pub(crate) const MIN_NODE_MAJOR: u32 = 18;

/// What to tell someone without a usable Node.js.
pub(crate) fn install_hint() -> String {
    format!(
        "`mcp-conformance test` runs the official suite, a Node.js package: it needs \
         Node.js {MIN_NODE_MAJOR} or newer, with npx, on PATH. Install it from \
         https://nodejs.org (or your platform's package manager) and run again. \
         Nothing was installed."
    )
}

/// npx's executable name: a batch file on Windows.
pub(crate) const fn npx() -> &'static str {
    if cfg!(windows) { "npx.cmd" } else { "npx" }
}

/// The major version in `node --version`'s output (`v22.22.0`).
pub(crate) fn node_major(output: &str) -> Option<u32> {
    output
        .trim()
        .strip_prefix('v')?
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// Checks that Node.js (new enough) and npx are on PATH.
///
/// # Errors
///
/// Either is missing or will not run, or Node is too old; the message says
/// what to install.
pub(crate) async fn check() -> Result<(), String> {
    let node = Command::new("node")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("cannot run `node` ({error}). {}", install_hint()))?;
    node_usable(
        node.status.success(),
        &String::from_utf8_lossy(&node.stdout),
    )?;
    let ran = Command::new(npx())
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("cannot run `{}` ({error}). {}", npx(), install_hint()))?;
    if !ran.status.success() {
        return Err(format!("`{} --version` failed. {}", npx(), install_hint()));
    }
    Ok(())
}

/// Whether `node --version`, which exited successfully or not and printed
/// `version`, is a Node.js new enough to run the suite.
fn node_usable(succeeded: bool, version: &str) -> Result<(), String> {
    match node_major(version) {
        Some(major) if succeeded && major >= MIN_NODE_MAJOR => Ok(()),
        _ => Err(format!(
            "`node --version` printed {:?}. {}",
            version.trim(),
            install_hint()
        )),
    }
}

/// The suite, as npx runs it from `work_dir`: `npx --yes <package>@<pin>`.
pub(crate) fn suite(work_dir: &Path) -> Command {
    let mut command = Command::new(npx());
    command
        .arg("--yes")
        .arg(format!("{}@{}", suite::PACKAGE, suite::VERSION))
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    command
}

/// Fetches the suite (on first use) and checks it reports the pinned version.
///
/// # Errors
///
/// npx failed, or the suite it ran reported another version.
pub(crate) async fn verify_suite(work_dir: &Path) -> Result<(), String> {
    eprintln!(
        "mcp-conformance: fetching {}@{} with npx (first run downloads it)",
        suite::PACKAGE,
        suite::VERSION
    );
    let output = suite(work_dir)
        .arg("--version")
        .stderr(Stdio::inherit())
        .output()
        .await
        .map_err(|error| format!("cannot run `{}` ({error}). {}", npx(), install_hint()))?;
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() {
        return Err(format!(
            "npx could not run {}@{} ({}): is the npm registry reachable?",
            suite::PACKAGE,
            suite::VERSION,
            output.status
        ));
    }
    if reported != suite::VERSION {
        return Err(format!(
            "npx ran {} reporting version {reported:?}, not the pinned {}; refusing to \
             run a suite other than the pinned one",
            suite::PACKAGE,
            suite::VERSION
        ));
    }
    Ok(())
}

/// Runs a prepared suite invocation to completion, its stdout forwarded to
/// this process's stderr (stdout carries the report), its stderr inherited.
/// Returns whether it exited successfully.
///
/// # Errors
///
/// The suite could not be started or waited for.
pub(crate) async fn run(mut command: Command) -> Result<bool, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("cannot run `{}` ({error}). {}", npx(), install_hint()))?;
    let forward = child.stdout.take().map(|stdout| {
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("suite: {line}");
            }
        })
    });
    let status = child
        .wait()
        .await
        .map_err(|error| format!("waiting for the suite failed: {error}"))?;
    if let Some(forward) = forward {
        let _ = forward.await;
    }
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_major_version_is_read_from_nodes_own_format() {
        assert_eq!(node_major("v22.22.0\n"), Some(22));
        assert_eq!(node_major("v18.0.0"), Some(18));
        assert_eq!(node_major("22.22.0"), None);
        assert_eq!(node_major("vX.1"), None);
        assert_eq!(node_major(""), None);
    }

    #[test]
    fn node_counts_when_it_ran_and_is_new_enough() {
        assert_eq!(node_usable(true, "v18.0.0\n"), Ok(()));
        assert_eq!(node_usable(true, "v22.22.0\n"), Ok(()));
        for (succeeded, version) in [(true, "v17.9.1\n"), (false, "v22.22.0\n"), (true, "")] {
            let Err(error) = node_usable(succeeded, version) else {
                panic!("{version:?} counted");
            };
            assert_eq!(
                error,
                format!(
                    "`node --version` printed {:?}. {}",
                    version.trim(),
                    install_hint()
                )
            );
        }
    }

    #[test]
    fn the_hint_names_the_requirement_and_promises_nothing_was_installed() {
        let hint = install_hint();
        assert!(hint.contains("Node.js 18 or newer"), "{hint}");
        assert!(hint.contains("npx"), "{hint}");
        assert!(hint.contains("https://nodejs.org"), "{hint}");
        assert!(hint.ends_with("Nothing was installed."), "{hint}");
    }

    #[test]
    fn the_suite_is_fetched_at_exactly_the_pin_from_the_work_dir() {
        let command = suite(Path::new("/work"));
        let command = command.as_std();
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--yes",
                &format!("@modelcontextprotocol/conformance@{}", suite::VERSION)
            ]
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("/work")));
        assert_eq!(command.get_program(), npx());
    }
}
