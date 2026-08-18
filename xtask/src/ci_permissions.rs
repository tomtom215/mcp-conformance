// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The CI write-scope gate: every `GITHUB_TOKEN` write permission in
//! `.github/workflows/` is granted on a job, and is listed in the security
//! model's inventory — both directions.
//!
//! `docs/plan/05-security-model.md` §"CI write scopes" tabulates which jobs
//! hold which write scopes and why. That table was written by hand, which
//! makes it the same kind of claim as the README's coverage counts: true the
//! day it was written and unenforced afterwards. A security document that
//! silently omits a scope somebody added is worse than no document, because
//! it is read as an inventory.
//!
//! Two rules, checked from the workflows themselves:
//!
//! 1. **No workflow-level write scope.** A top-level `permissions:` block
//!    applies to every job in the file, including ones added later that never
//!    asked for it. Write scopes belong on the job that writes.
//! 2. **The table and the workflows name the same set.** Every job-level
//!    write scope appears as a row, and every row corresponds to a scope that
//!    exists. A removed scope leaves a stale row; an added one leaves a gap.
//!
//! Read scopes are deliberately out of scope: `contents: read` is the default
//! posture, adding one grants nothing, and tabulating them would bury the
//! five rows that matter under a dozen that do not.
//!
//! The workflow scan is line-oriented rather than a YAML parse. The workspace
//! carries no YAML dependency and will not add one for this (the obvious
//! crate is unmaintained, which `cargo audit` would rightly complain about),
//! and the shape being read — a `permissions:` mapping of `scope: value` — is
//! the most regular corner of the format. Anything the scan does not
//! recognise fails the gate rather than being skipped.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::BTreeSet;
use std::path::Path;

/// The security model, relative to the workspace root.
const MODEL: &str = "docs/plan/05-security-model.md";

/// The section whose table is the inventory.
const SECTION: &str = "## CI write scopes";

/// One granted write scope: which workflow file, which job (`None` for a
/// workflow-level block), which permission.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Scope {
    workflow: String,
    job: Option<String>,
    permission: String,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.job {
            Some(job) => write!(
                out,
                "{}: job {job}: {}: write",
                self.workflow, self.permission
            ),
            None => write!(
                out,
                "{}: workflow-level {}: write",
                self.workflow, self.permission
            ),
        }
    }
}

/// Runs the gate; `true` when both rules hold.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    let Some(files) = workflow_files(&root.join(".github/workflows")) else {
        return false;
    };
    let Some(granted) = granted_scopes(&files) else {
        return false;
    };
    let listed = match std::fs::read_to_string(root.join(MODEL)) {
        Ok(text) => match table_scopes(&text) {
            Ok(listed) => listed,
            Err(complaint) => {
                eprintln!("xtask: ci-permissions — {MODEL}: {complaint}");
                return false;
            }
        },
        Err(error) => {
            eprintln!("xtask: ci-permissions — cannot read {MODEL}: {error}");
            return false;
        }
    };
    let mut ok = true;
    for complaint in complaints(&granted, &listed) {
        eprintln!("xtask: ci-permissions — {complaint}");
        ok = false;
    }
    if ok {
        eprintln!(
            "xtask: ci-permissions — {} write scope(s) across {} workflow(s), each on a job \
             and each listed in {MODEL}",
            granted.len(),
            files.len()
        );
    }
    ok
}

/// The workflow files to read, sorted. `None`, having said why, when the
/// directory cannot be read or holds none: a gate that scanned nothing proves
/// nothing.
fn workflow_files(dir: &Path) -> Option<Vec<std::path::PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!(
                "xtask: ci-permissions — cannot read {}: {error}",
                dir.display()
            );
            return None;
        }
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
        .collect();
    files.sort();
    if files.is_empty() {
        eprintln!(
            "xtask: ci-permissions — no workflows found under {}",
            dir.display()
        );
        return None;
    }
    Some(files)
}

/// Every write scope the given workflows grant. `None`, having said why, when
/// one of them cannot be read or parsed.
fn granted_scopes(files: &[std::path::PathBuf]) -> Option<BTreeSet<Scope>> {
    let mut granted = BTreeSet::new();
    for path in files {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("xtask: ci-permissions — cannot read {}", path.display());
            return None;
        };
        match scopes_in(&name, &text) {
            Ok(scopes) => granted.extend(scopes),
            Err(complaint) => {
                eprintln!("xtask: ci-permissions — {complaint}");
                return None;
            }
        }
    }
    Some(granted)
}

/// Everything wrong with the two sets, in the order a reader wants it: scopes
/// granted workflow-wide first — a rule about the workflows alone — then the
/// two directions in which the inventory and the workflows can disagree.
fn complaints(granted: &BTreeSet<Scope>, listed: &BTreeSet<Scope>) -> Vec<String> {
    let mut complaints = Vec::new();
    for scope in granted.iter().filter(|scope| scope.job.is_none()) {
        complaints.push(format!(
            "{scope} applies to every job in the file, including ones added later; \
             move it onto the job that writes"
        ));
    }
    if !complaints.is_empty() {
        // The inventory tabulates jobs, so it cannot describe a workflow-level
        // grant either way; reporting both would bury the rule that matters.
        return complaints;
    }
    for scope in granted.difference(listed) {
        complaints.push(format!(
            "{scope} is granted but absent from {MODEL} §\"CI write scopes\"; \
             a scope nobody wrote down is a scope nobody reviews"
        ));
    }
    for scope in listed.difference(granted) {
        complaints.push(format!(
            "{MODEL} lists {scope}, which no workflow grants; an inventory that \
             over-states is read as one that is maintained"
        ));
    }
    complaints
}

/// Every write scope one workflow grants.
///
/// The scan recognises exactly two `permissions:` positions — column 0
/// (workflow-level) and column 4 (a job's, under `jobs:` → job key at column
/// 2) — and refuses anything else rather than skipping it.
fn scopes_in(workflow: &str, text: &str) -> Result<Vec<Scope>, String> {
    let mut scopes = Vec::new();
    let mut job: Option<String> = None;
    let mut block: Option<(usize, Option<String>)> = None;
    for (number, line) in text.lines().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // A job key: two-space indent, ends in a colon, inside `jobs:`.
        if indent == 2 && trimmed.ends_with(':') && !trimmed.contains(' ') {
            job = Some(trimmed.trim_end_matches(':').to_owned());
        }
        if let Some((block_indent, owner)) = &block {
            if indent > *block_indent {
                if let Some(permission) = write_permission(trimmed) {
                    scopes.push(Scope {
                        workflow: workflow.to_owned(),
                        job: owner.clone(),
                        permission,
                    });
                }
                continue;
            }
            block = None;
        }
        let Some(rest) = trimmed.strip_prefix("permissions:") else {
            continue;
        };
        let owner = match indent {
            0 => None,
            4 => Some(job.clone().ok_or_else(|| {
                format!(
                    "{workflow}:{}: a job's permissions with no job above it",
                    number + 1
                )
            })?),
            other => {
                return Err(format!(
                    "{workflow}:{}: `permissions:` at indent {other}; this gate reads indent 0 \
                     (workflow) and 4 (job) and will not guess at anything else",
                    number + 1
                ));
            }
        };
        // `permissions: {}` and `permissions: write-all` are inline forms.
        let rest = strip_comment(rest).trim();
        if rest.is_empty() {
            block = Some((indent, owner));
        } else if rest != "{}" {
            return Err(format!(
                "{workflow}:{}: inline `permissions: {rest}`; only the empty `{{}}` form and \
                 block form are understood",
                number + 1
            ));
        }
    }
    Ok(scopes)
}

/// The permission name from a `name: write` entry, if that is what it is.
fn write_permission(entry: &str) -> Option<String> {
    let (name, value) = entry.split_once(':')?;
    (strip_comment(value).trim() == "write").then(|| name.trim().to_owned())
}

/// Everything before an unquoted `#` (permission values never contain one).
fn strip_comment(text: &str) -> &str {
    text.split('#').next().unwrap_or(text)
}

/// The scopes the security model's inventory table lists.
///
/// Rows look like `| `release.yml` (…) | `package` | `id-token`, `attestations` | … |`:
/// the first backticked token of column one names the workflow, column two the
/// job, and every backticked token of column three a permission. A row whose
/// workflow cell carries no file name repeats the one above it, as the table
/// does for `release.yml`'s three jobs.
fn table_scopes(model: &str) -> Result<BTreeSet<Scope>, String> {
    let section = model
        .split_once(SECTION)
        .ok_or_else(|| format!("no {SECTION:?} section"))?
        .1;
    let section = section.split("\n## ").next().unwrap_or(section);
    let mut listed = BTreeSet::new();
    let mut workflow: Option<String> = None;
    for line in section.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.contains("---") {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 4 || cells[0] == "Workflow (triggers)" {
            continue;
        }
        if let Some(name) = backticked(cells[0])
            .into_iter()
            .find(|cell| Path::new(cell).extension().is_some_and(|ext| ext == "yml"))
        {
            workflow = Some(name);
        }
        let workflow = workflow
            .clone()
            .ok_or_else(|| format!("row {line:?} names no workflow, and none precedes it"))?;
        let job = backticked(cells[1])
            .into_iter()
            .next()
            .ok_or_else(|| format!("row {line:?} names no job"))?;
        let permissions = backticked(cells[2]);
        if permissions.is_empty() {
            return Err(format!("row {line:?} names no permission"));
        }
        for permission in permissions {
            listed.insert(Scope {
                workflow: workflow.clone(),
                job: Some(job.clone()),
                permission,
            });
        }
    }
    if listed.is_empty() {
        return Err(format!("{SECTION:?} contains no inventory rows"));
    }
    Ok(listed)
}

/// The contents of every `` `backticked` `` span in a table cell.
fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const JOB_LEVEL: &str = "\
name: X
permissions:
  contents: read
jobs:
  build:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      id-token: write # OIDC
      attestations: write
    steps:
      - run: echo permissions:
  other:
    runs-on: ubuntu-latest
    steps:
      - run: true
";

    #[test]
    fn a_jobs_write_scopes_are_found_and_attributed_to_that_job() {
        let scopes = scopes_in("x.yml", JOB_LEVEL).unwrap();
        let names: Vec<&str> = scopes.iter().map(|s| s.permission.as_str()).collect();
        assert_eq!(names, ["id-token", "attestations"]);
        assert!(
            scopes.iter().all(|s| s.job.as_deref() == Some("build")),
            "{scopes:?}"
        );
    }

    #[test]
    fn a_workflow_level_write_scope_is_attributed_to_no_job() {
        let text = "permissions:\n  issues: write\njobs:\n  a:\n    runs-on: x\n";
        let scopes = scopes_in("x.yml", text).unwrap();
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].job, None);
        assert!(
            scopes[0].to_string().contains("workflow-level"),
            "{}",
            scopes[0]
        );
    }

    #[test]
    fn read_scopes_and_the_empty_form_grant_nothing() {
        assert!(
            scopes_in("x.yml", "permissions: {}\njobs:\n  a:\n    runs-on: x\n")
                .unwrap()
                .is_empty()
        );
        assert!(
            scopes_in("x.yml", "permissions:\n  contents: read\n")
                .unwrap()
                .is_empty()
        );
        assert_eq!(write_permission("contents: read"), None);
        assert_eq!(
            write_permission("issues: write # why"),
            Some("issues".to_owned())
        );
        assert_eq!(write_permission("- run: true"), None);
    }

    #[test]
    fn a_permissions_block_the_scan_cannot_place_fails_rather_than_being_skipped() {
        let odd = "jobs:\n  a:\n    steps:\n      permissions:\n        issues: write\n";
        assert!(scopes_in("x.yml", odd).unwrap_err().contains("indent 6"));
        let inline = "permissions: write-all\n";
        assert!(
            scopes_in("x.yml", inline)
                .unwrap_err()
                .contains("write-all")
        );
    }

    #[test]
    fn the_table_parses_repeated_workflow_cells_and_multi_scope_cells() {
        let model = "\
## CI write scopes (reviewed 2026-08-18)

| Workflow (triggers) | Job | Write scope | For |
|---|---|---|---|
| `release.yml` (`push`) | `package` | `id-token`, `attestations` | a |
| `release.yml` | `github-release` | `contents` | b |

## Next
";
        let listed = table_scopes(model).unwrap();
        assert_eq!(listed.len(), 3);
        assert!(
            listed.iter().all(|s| s.workflow == "release.yml"),
            "{listed:?}"
        );
        let jobs: BTreeSet<_> = listed.iter().filter_map(|s| s.job.clone()).collect();
        assert_eq!(jobs.len(), 2);
    }

    #[test]
    fn a_table_that_is_missing_or_empty_is_a_failure_not_an_empty_inventory() {
        assert!(table_scopes("# nothing here").is_err());
        assert!(table_scopes("## CI write scopes\n\nprose only\n").is_err());
    }

    #[test]
    fn the_committed_workflows_and_the_committed_table_agree() {
        // The gate's own end-to-end contract, so a rewrite of either parser
        // cannot pass its unit tests while disagreeing with the repository.
        assert!(run());
    }
}
