// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask draft-readiness` — how much of the next revision the
//! everything server already satisfies, measured rather than estimated.
//!
//! The `2026-07-28` rework (SEP-2575 and friends) is inventoried in
//! `docs/plan/01-ecosystem-context.md` rows 1.5a–1.5f as a *list of changes*.
//! A list says what will be different; it does not say how much work the
//! migration is. This task answers that with a number: it drives the official
//! runner's **draft scenario set** against the current `2025-11-25` server and
//! records the score against a committed baseline
//! (`conformance/draft-readiness.json`).
//!
//! The baseline makes the number a **ratchet**, not a report. The gate fails
//! when the score drops (a migration regression) *and* when it rises or the
//! scenario set changes (`BLESS=1` to re-record) — so the figure quoted in the
//! roadmap cannot silently drift in either direction, the same contract the
//! coverage manifest already has.
//!
//! Deliberately **not** part of `cargo xtask conformance`: that task judges the
//! server against the revision the registry implements and reconciles the
//! runner's verdicts with our validator's. Here the runner speaks a revision
//! the registry does not describe yet, so there is nothing to reconcile
//! against — replaying these sessions through a `2025-11-25` registry would
//! produce a verdict about the wrong specification. This task therefore runs
//! the runner leg only, and the failures it records are expected findings, not
//! build breakage.
//!
//! Like `conformance`, this is orchestration: it may use the network (npm for
//! the runner) and real sockets, which `cargo test` never does.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, ExitCode};

/// The suite version this task pins. The **draft** scenarios only exist on the
/// `0.2.0-alpha` line ([register 2.4](../../docs/plan/01-ecosystem-context.md)),
/// but the pin is exact rather than the floating `alpha` dist-tag: a ratchet
/// whose input can change under it is not a ratchet. Bumps are deliberate —
/// re-measure, re-bless, and update register row 2.4 in the same commit.
pub(super) const DRAFT_SUITE_VERSION: &str = "0.2.0-alpha.9";

/// The revision under test — the one the registry does not describe yet.
pub(super) const DRAFT_SPEC_VERSION: &str = "2026-07-28";

/// Committed score, relative to the workspace root.
pub(super) const BASELINE: &str = "conformance/draft-readiness.json";

/// Scratch space for this task's runner output, kept away from
/// `target/conformance/` so a draft run can never be mistaken for — or clobber
/// — the pinned-revision artifacts the agreement check reads.
const RESULTS_DIR: &str = "target/draft-readiness";

/// One scenario's measured outcome: every check the runner emitted, by id,
/// with the status it reported.
///
/// Statuses are kept verbatim rather than folded into a passed/total ratio.
/// The runner emits `INFO` for checks that are informational at this
/// revision, and counting those in a denominator would report "not
/// applicable" as "failing" — the same vacuous accounting the registry
/// refuses for capability-gated requirements
/// ([ADR-0006](../../docs/plan/decisions/0006-capability-gated-applicability.md)).
pub(super) type Scenario = BTreeMap<String, String>;

pub(crate) fn run(bless: bool) -> ExitCode {
    let root = crate::workspace_root();
    let Some(results_dir) = prepare(&root) else {
        return ExitCode::FAILURE;
    };

    // The tap is irrelevant here (nothing replays these sessions), but
    // `start_server` owns the readiness handshake, so reuse it and give the
    // recordings their own scratch directory.
    let tap_dir = results_dir.join("tap");
    let Some((mut server, address)) = crate::conformance::start_server(&root, &tap_dir) else {
        return ExitCode::FAILURE;
    };

    let ran = run_draft_suite(&root, &results_dir, &address);
    let _ = server.kill();
    let _ = server.wait();
    if !ran {
        return ExitCode::FAILURE;
    }

    let measured = match collect(&results_dir) {
        Ok(scores) if scores.is_empty() => {
            eprintln!(
                "xtask: draft-readiness — the runner wrote no scenario results; \
                 treating as a harness failure rather than a score of zero"
            );
            return ExitCode::FAILURE;
        }
        Ok(scores) => scores,
        Err(message) => {
            eprintln!("xtask: draft-readiness — {message}");
            return ExitCode::FAILURE;
        }
    };
    report(&measured);

    if bless {
        return match baseline::write(&root.join(BASELINE), &measured) {
            Ok(()) => {
                eprintln!("xtask: draft-readiness — baseline re-recorded ({BASELINE})");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("xtask: draft-readiness — {message}");
                ExitCode::FAILURE
            }
        };
    }

    match baseline::read(&root.join(BASELINE)) {
        Ok(recorded) => baseline::compare(&recorded, &measured),
        Err(message) => {
            eprintln!("xtask: draft-readiness — {message}");
            ExitCode::FAILURE
        }
    }
}

/// Builds the server and clears the previous run's artifacts, yielding the
/// results directory. `None` means the run cannot proceed.
fn prepare(root: &Path) -> Option<std::path::PathBuf> {
    eprintln!("xtask: draft-readiness — building mcp-everything-server");
    let build = Command::new("cargo")
        .args(["build", "-p", "mcp-everything-server", "--all-features"])
        .current_dir(root)
        .status();
    if !matches!(build, Ok(status) if status.success()) {
        eprintln!("xtask: draft-readiness — server build failed");
        return None;
    }
    // Fresh artifacts every run: a scenario the runner skips this time must
    // not be scored from last time's leftovers.
    let results_dir = root.join(RESULTS_DIR);
    if let Err(error) = std::fs::remove_dir_all(&results_dir)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "xtask: draft-readiness — cannot clear {}: {error}",
            results_dir.display()
        );
        return None;
    }
    Some(results_dir)
}

/// Drives the runner's draft scenario set. Returns whether the runner *ran* —
/// its exit status is deliberately ignored, because scenario failures are this
/// task's measurement, not its verdict.
fn run_draft_suite(root: &Path, results_dir: &Path, address: &str) -> bool {
    eprintln!(
        "xtask: draft-readiness — running @modelcontextprotocol/conformance@{DRAFT_SUITE_VERSION} \
         (spec {DRAFT_SPEC_VERSION}) against http://{address}/mcp"
    );
    let status = Command::new("npx")
        .arg("-y")
        .arg(format!(
            "@modelcontextprotocol/conformance@{DRAFT_SUITE_VERSION}"
        ))
        .arg("server")
        .arg("--url")
        .arg(format!("http://{address}/mcp"))
        .arg("--spec-version")
        .arg(DRAFT_SPEC_VERSION)
        .arg("--output-dir")
        .arg(results_dir)
        .current_dir(root)
        .status();
    match status {
        Ok(_) => true,
        Err(error) => {
            eprintln!(
                "xtask: draft-readiness — could not run npx ({error}). \
                 Node.js is required for this task."
            );
            false
        }
    }
}

/// Reads every `server-<scenario>-<timestamp>/checks.json` the runner wrote and
/// folds it into one score per scenario. The timestamp is stripped so the
/// baseline compares scenarios, not run instants.
fn collect(results_dir: &Path) -> Result<BTreeMap<String, Scenario>, String> {
    let mut scores: BTreeMap<String, Scenario> = BTreeMap::new();
    let entries = std::fs::read_dir(results_dir)
        .map_err(|error| format!("cannot read {}: {error}", results_dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(scenario) = scenario_name(&name) else {
            continue;
        };
        let checks_path = entry.path().join("checks.json");
        let Ok(body) = std::fs::read_to_string(&checks_path) else {
            continue;
        };
        let checks: Vec<serde_json::Value> = serde_json::from_str(&body)
            .map_err(|error| format!("cannot parse {}: {error}", checks_path.display()))?;
        // A scenario the runner retried overwrites rather than accumulates.
        let entry = scores.entry(scenario).or_default();
        entry.clear();
        for check in &checks {
            let (Some(id), Some(status)) = (
                check.get("id").and_then(serde_json::Value::as_str),
                check.get("status").and_then(serde_json::Value::as_str),
            ) else {
                return Err(format!(
                    "a check in {} has no id/status pair",
                    checks_path.display()
                ));
            };
            entry.insert(id.to_owned(), status.to_owned());
        }
    }
    Ok(scores)
}

/// `server-tools-list-2026-07-26T22-55-17-793Z` → `tools-list`.
///
/// The runner appends an ISO-8601 instant with `:` and `.` rewritten as `-`,
/// giving a suffix of exactly `-YYYY-MM-DDTHH-MM-SS-mmmZ`. Matching that whole
/// fixed shape at the end is the only unambiguous rule: searching for the
/// first `-<4 digits>-` cuts a scenario like `sep-1034-…` down to `sep`, and
/// searching from the right cuts *inside* the timestamp (`-26T22` also opens
/// with `-2`), leaving `tools-list-2026-07`. Both bugs are pinned by tests.
fn scenario_name(dir_name: &str) -> Option<String> {
    let rest = dir_name.strip_prefix("server-")?;
    let cut = rest.len().checked_sub(TIMESTAMP_LEN)?;
    // `is_char_boundary` keeps the slicing total for non-ASCII input; the
    // shape check then rejects anything that is not the runner's timestamp.
    if !rest.is_char_boundary(cut) || !is_timestamp(&rest[cut..]) {
        return None;
    }
    let name = &rest[..cut];
    (!name.is_empty()).then(|| name.to_owned())
}

/// Length of `-YYYY-MM-DDTHH-MM-SS-mmmZ`.
const TIMESTAMP_LEN: usize = 25;

/// Whether `candidate` is exactly the runner's `-YYYY-MM-DDTHH-MM-SS-mmmZ`.
fn is_timestamp(candidate: &str) -> bool {
    /// (index, expected literal) for every non-digit position.
    const PUNCTUATION: [(usize, u8); 8] = [
        (0, b'-'),
        (5, b'-'),
        (8, b'-'),
        (11, b'T'),
        (14, b'-'),
        (17, b'-'),
        (20, b'-'),
        (24, b'Z'),
    ];
    let bytes = candidate.as_bytes();
    if bytes.len() != TIMESTAMP_LEN {
        return false;
    }
    PUNCTUATION
        .iter()
        .all(|&(index, expected)| bytes[index] == expected)
        && bytes.iter().enumerate().all(|(index, byte)| {
            PUNCTUATION.iter().any(|&(at, _)| at == index) || byte.is_ascii_digit()
        })
}

/// Prints the measured picture: passing checks, failing checks, and the
/// informational ones kept separate from both.
fn report(measured: &BTreeMap<String, Scenario>) {
    let tally = |wanted: &str| -> usize {
        measured
            .values()
            .flat_map(BTreeMap::values)
            .filter(|status| status.as_str() == wanted)
            .count()
    };
    let (passing, failing, informational) = (tally(SUCCESS), tally(FAILURE), tally(INFO));
    eprintln!(
        "xtask: draft-readiness — {passing} passing, {failing} failing, {informational} \
         informational across {} scenario(s) at spec {DRAFT_SPEC_VERSION} \
         (suite {DRAFT_SUITE_VERSION})",
        measured.len()
    );
    for (scenario, checks) in measured {
        for (check, status) in checks {
            if status != SUCCESS {
                eprintln!("xtask: draft-readiness —   {scenario} / {check}: {status}");
            }
        }
    }
}

/// The runner's spellings, kept as constants so a rename upstream is one edit.
pub(super) const SUCCESS: &str = "SUCCESS";
pub(super) const FAILURE: &str = "FAILURE";
pub(super) const INFO: &str = "INFO";

mod baseline;

#[cfg(test)]
mod tests {
    use super::scenario_name;

    #[test]
    fn scenario_name_strips_the_prefix_and_the_whole_timestamp() {
        assert_eq!(
            scenario_name("server-tools-list-2026-07-26T22-55-17-793Z").as_deref(),
            Some("tools-list")
        );
        // Regression pin: searching from the right cuts inside the timestamp
        // (`-26T22` also opens with `-2`) and yields `…-2026-07`.
        assert_eq!(
            scenario_name("server-dns-rebinding-protection-2026-07-26T22-55-19-456Z").as_deref(),
            Some("dns-rebinding-protection")
        );
        // A scenario name that itself ends in digits must survive.
        assert_eq!(
            scenario_name("server-sep-1034-2026-07-26T22-55-19-456Z").as_deref(),
            Some("sep-1034")
        );
    }

    #[test]
    fn non_scenario_directories_are_ignored() {
        assert_eq!(scenario_name("tap"), None);
        assert_eq!(scenario_name("client"), None);
        assert_eq!(scenario_name("server-"), None);
        // A `server-` directory with no timestamp is not a scenario result.
        assert_eq!(scenario_name("server-tools-list"), None);
    }
}
