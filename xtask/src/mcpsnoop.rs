// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask mcpsnoop [--check]` — the second calibration source.
//!
//! [`mcpsnoop`](https://github.com/kerlenton/mcpsnoop) is the one other tool
//! that judges recorded MCP sessions offline (`mcpsnoop check`, register row
//! 5.10). Holding our verdicts against its verdicts on the same sessions is a
//! calibration the official suite cannot give: the suite drives live
//! scenarios, while mcpsnoop, like us, reads a recording.
//!
//! The bare task **measures**: it needs the pinned binary at `$MCPSNOOP`
//! (skipping loudly without it), converts every corpus trace into mcpsnoop's
//! capture format (`convert.rs`, which says why that is faithful), runs
//! `mcpsnoop check` on each, and writes `conformance/mcpsnoop/results.json`.
//! Then it renders `book/src/mcpsnoop.md` from the results, the hand-read rule
//! map (`rules.json`) and the hand-written triage (`triage.json`).
//!
//! `--render` re-renders the page from the committed files alone, for a
//! triage or rule-map edit. `--check` needs no binary either, and runs in `cargo xtask gates`: it re-renders the
//! page from the three committed files and fails when the committed page
//! differs, when a divergence has no triage or a triage entry no longer
//! matches a divergence, when a finding belongs to no rule, or when a mapped or
//! quoted requirement is not in its revision's registry. The page cannot be
//! edited by hand, and the triage cannot fall behind the results.
//!
//! mcpsnoop never gates a pull request: the measurement is informational,
//! scheduled, and pinned (`PINNED`), and `--check` reads only committed files.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mcp_conformance_core::requirement::RegistrySet;
use mcp_conformance_core::wire_schema::AUTHORITY_QUOTE;

mod classify;
mod convert;
mod data;
mod measure;
mod render;
mod verify;

use data::{Results, Rules, Triage, load};

/// The mcpsnoop release the committed results were measured with.
pub(crate) const PINNED: &str = "0.26.1";

/// The pseudo-requirement our reports' wire-schema verdict stands for.
pub(crate) const WIRE_SCHEMA: &str = "wire-schema";

const RESULTS: &str = "conformance/mcpsnoop/results.json";
const RULES: &str = "conformance/mcpsnoop/rules.json";
const TRIAGE: &str = "conformance/mcpsnoop/triage.json";
const PAGE: &str = "book/src/mcpsnoop.md";

/// Dispatches `cargo xtask mcpsnoop [--check]`.
pub(crate) fn run(args: &[String]) -> bool {
    match args {
        [] => measure_and_render(),
        [flag] if flag == "--check" => check_gate(),
        [flag] if flag == "--render" => render(&crate::workspace_root()),
        _ => {
            eprintln!("xtask: mcpsnoop — expected no argument, --check or --render, got {args:?}");
            false
        }
    }
}

/// The `gates` entry: the committed page, results, rules and triage agree.
pub(crate) fn check_gate() -> bool {
    eprintln!("xtask: mcpsnoop divergence table in sync — cargo xtask mcpsnoop --check");
    let root = crate::workspace_root();
    match committed(&root).and_then(|inputs| verify::verify(&root, &inputs)) {
        Ok(rendered) => {
            let page = root.join(PAGE);
            let committed = fs::read_to_string(&page).unwrap_or_default();
            if committed == rendered {
                eprintln!("xtask: mcpsnoop — {PAGE} matches the committed results and triage");
                true
            } else {
                eprintln!(
                    "xtask: mcpsnoop — {PAGE} differs from what the committed results, rules \
                     and triage render; regenerate it with `cargo xtask mcpsnoop` (with \
                     MCPSNOOP set) rather than editing it"
                );
                false
            }
        }
        Err(problems) => {
            for problem in problems {
                eprintln!("xtask: mcpsnoop — {problem}");
            }
            false
        }
    }
}

fn measure_and_render() -> bool {
    let Some(binary) = std::env::var_os("MCPSNOOP").map(PathBuf::from) else {
        eprintln!(
            "xtask: mcpsnoop — SKIPPED: set MCPSNOOP to an mcpsnoop {PINNED} binary to measure \
             (book/src/mcpsnoop.md says how to fetch and verify one). Nothing was compared; the \
             committed results stand unchanged."
        );
        return true;
    };
    let root = crate::workspace_root();
    let version = match binary_version(&binary) {
        Ok(version) if version == PINNED => version,
        Ok(version) => {
            eprintln!(
                "xtask: mcpsnoop — {} is {version}; the pin is {PINNED}",
                binary.display()
            );
            return false;
        }
        Err(error) => {
            eprintln!("xtask: mcpsnoop — {error}");
            return false;
        }
    };
    let results = match measure::measure(&root, &binary, &version) {
        Ok(results) => results,
        Err(error) => {
            eprintln!("xtask: mcpsnoop — {error}");
            return false;
        }
    };
    let mut text = serde_json::to_string_pretty(&results).unwrap_or_default();
    text.push('\n');
    if let Err(error) = fs::write(root.join(RESULTS), text) {
        eprintln!("xtask: mcpsnoop — cannot write {RESULTS}: {error}");
        return false;
    }
    eprintln!(
        "xtask: mcpsnoop — measured {} traces into {RESULTS}",
        results.traces.len()
    );
    render(&root)
}

/// Renders the page from the committed files; no binary needed, so a triage
/// edit can be rendered by anyone.
fn render(root: &Path) -> bool {
    match committed(root).and_then(|inputs| verify::verify(root, &inputs)) {
        Ok(rendered) => match fs::write(root.join(PAGE), rendered) {
            Ok(()) => {
                eprintln!("xtask: mcpsnoop — wrote {PAGE}");
                true
            }
            Err(error) => {
                eprintln!("xtask: mcpsnoop — cannot write {PAGE}: {error}");
                false
            }
        },
        Err(problems) => {
            for problem in problems {
                eprintln!("xtask: mcpsnoop — {problem}");
            }
            eprintln!("xtask: mcpsnoop — triage the above, then `cargo xtask mcpsnoop --render`");
            false
        }
    }
}

/// `mcpsnoop --version` → `0.26.1`.
fn binary_version(binary: &Path) -> Result<String, String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot run {}: {error}", binary.display()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .nth(1)
        .map(str::to_owned)
        .ok_or_else(|| format!("{} --version printed {text:?}", binary.display()))
}

/// Everything the page is a projection of.
pub(crate) struct Inputs {
    pub results: Results,
    pub rules: Rules,
    pub triage: Triage,
    pub quotes: Quotes,
}

fn committed(root: &Path) -> Result<Inputs, Vec<String>> {
    let results = load(&root.join(RESULTS)).map_err(|error| vec![error])?;
    let rules = load(&root.join(RULES)).map_err(|error| vec![error])?;
    let triage = load(&root.join(TRIAGE)).map_err(|error| vec![error])?;
    let quotes = Quotes::builtin().map_err(|error| vec![error])?;
    Ok(Inputs {
        results,
        rules,
        triage,
        quotes,
    })
}

/// A clause as the page quotes it.
#[derive(Debug, Clone)]
pub(crate) struct Clause {
    pub level: String,
    pub quote: String,
    pub url: String,
}

/// Every clause of every revision, by `(revision, id)`.
#[derive(Debug, Default)]
pub(crate) struct Quotes {
    clauses: BTreeMap<(String, String), Clause>,
}

impl Quotes {
    fn builtin() -> Result<Self, String> {
        let set = RegistrySet::builtin().map_err(|error| format!("registry: {error}"))?;
        let mut clauses = BTreeMap::new();
        for revision in set.revisions() {
            let registry = set
                .registry(*revision)
                .ok_or_else(|| format!("the registry set does not project to {revision}"))?;
            for requirement in registry.requirements() {
                clauses.insert(
                    (revision.to_string(), requirement.id.as_str().to_owned()),
                    Clause {
                        level: requirement.level.keyword().to_owned(),
                        quote: requirement.source.quote.clone(),
                        url: requirement.source.url(*revision),
                    },
                );
            }
            clauses.insert(
                (revision.to_string(), WIRE_SCHEMA.to_owned()),
                Clause {
                    level: "schema".to_owned(),
                    quote: AUTHORITY_QUOTE.to_owned(),
                    url: format!(
                        "https://modelcontextprotocol.io/specification/{revision}/basic#schema"
                    ),
                },
            );
        }
        Ok(Self { clauses })
    }

    /// The clause `id` names at `revision`.
    pub(crate) fn get(&self, revision: &str, id: &str) -> Option<&Clause> {
        self.clauses.get(&(revision.to_owned(), id.to_owned()))
    }

    /// The revisions the registry describes.
    pub(crate) fn revisions(&self) -> BTreeSet<&str> {
        self.clauses
            .keys()
            .map(|(revision, _)| revision.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests;
