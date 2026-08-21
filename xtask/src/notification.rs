// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `claims-expire` notification steps, executed.
//!
//! ADR-0010's 2026-08-18 amendment ends the weekly gates in a notification: a
//! red run opens or comments on a tracking issue naming the expired ledger
//! rows, a green one closes it. That is the last mile of the mechanism, and
//! until this module existed it was the only part of it nothing re-ran. It was
//! checked once, by hand, against a `gh` stub that was never committed — which
//! is the exact shape of claim ADR-0010 was written against, one layer further
//! out again.
//!
//! It also fails quietly when it fails. The steps run **inside a job that is
//! already red** (`if: failure()`), so a broken `gh` invocation adds one more
//! red step to a run whose colour was never going to change, and the visible
//! symptom is an issue that simply never appears. Nobody is watching for the
//! absence of a notification; that is what a notification is for.
//!
//! So the committed shell is lifted out of `scheduled.yml` and run here, under
//! stubbed `gh` and `cargo`, over the branches that matter: rows expired, no
//! rows but a red ledger, a drift failure, a moved pin, an issue that already
//! exists, and the green close. The script under test is the one the workflow
//! ships — read from the YAML, not transcribed — so it cannot drift from what
//! CI runs, and renaming a step fails the test rather than silently testing
//! nothing.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};

/// The workflow the claims-expire job lives in.
pub(crate) const WORKFLOW: &str = ".github/workflows/scheduled.yml";

/// The step whose script composes and posts the tracking issue.
pub(crate) const OPEN_STEP: &str = "Open or update the tracking issue";

/// The step that closes it once every gate is green.
pub(crate) const CLOSE_STEP: &str = "Close the tracking issue once all three gates are green";

/// The `run:` block of the named step, dedented to a runnable script.
///
/// Returns `None` when no step carries that name — which is a failure, not an
/// absence: a renamed step must break the tests that exercise it rather than
/// leave them passing over nothing.
pub(crate) fn step_script(workflow: &str, step: &str) -> Option<String> {
    let lines: Vec<&str> = workflow.lines().collect();
    let name = format!("- name: {step}");
    let start = lines.iter().position(|line| line.trim() == name)?;
    let run = lines
        .iter()
        .skip(start)
        .position(|line| line.trim() == "run: |")?
        + start;
    let indent = lines[run].len() - lines[run].trim_start().len() + 2;

    let mut script = String::new();
    for line in lines.iter().skip(run + 1) {
        let blank = line.trim().is_empty();
        if !blank && line.len() - line.trim_start().len() < indent {
            break;
        }
        script.push_str(if blank { "" } else { &line[indent..] });
        script.push('\n');
    }
    Some(script)
}

/// The workspace root, so tests read the committed workflow rather than a copy.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod harness;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
