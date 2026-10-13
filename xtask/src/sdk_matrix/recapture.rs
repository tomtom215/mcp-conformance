// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask sdk-matrix --recaptured DIR`: does a fresh `matrix.sh` run
//! still say what the committed corpus says?
//!
//! The scheduled job re-records every cell with the pinned SDKs and runs this,
//! so the published matrix cannot quietly stop being true. Each fresh trace is
//! judged in-process against its column's registry — the same judgment the
//! golden suite makes — and compared with the committed golden, row by row,
//! finding by finding.
//!
//! **The comparison ignores each finding's `seq` and nothing else.** A session
//! recorded over HTTP is the same session when one server's reply is written a
//! line earlier, and the trace bytes themselves differ on every run (each
//! server mints a fresh session id), so neither is evidence of a changed
//! result; which requirements fire, and what each finding says, is.
//!
//! With `BLESS=1` the run's traces replace the committed ones instead, which
//! is the deliberate act a real change calls for: then `cargo xtask bless` and
//! `cargo xtask sdk-matrix`, and the golden diff is reviewed like code.

use std::fs;
use std::path::Path;

use mcp_conformance_core::requirement::RegistrySet;
use serde_json::Value;

use super::{REVISIONS, Revision, SDKS, stem};

/// What `matrix.sh` names a session whose server negotiated an older revision.
const NOT_SERVED: &str = "not-served.jsonl";

/// Compares (or, blessing, adopts) every cell of the run in `dir`.
pub(super) fn run(root: &Path, dir: &Path, bless: bool) -> bool {
    let mut ok = true;
    for sdk in &SDKS {
        for revision in &REVISIONS {
            let stem = stem(sdk, revision);
            let outcome = if bless {
                adopt(root, dir, revision, &stem)
            } else {
                compare(root, dir, revision, &stem)
            };
            match outcome {
                Ok(line) => eprintln!("xtask: sdk-matrix — {stem}: {line}"),
                Err(problem) => {
                    eprintln!("xtask: sdk-matrix — {stem}: {problem}");
                    ok = false;
                }
            }
        }
    }
    if ok && bless {
        eprintln!(
            "xtask: sdk-matrix — traces adopted; now `cargo xtask bless`, `cargo xtask \
             sdk-matrix` and `cargo xtask draft-coverage`, and review the golden diff"
        );
    } else if !ok && !bless {
        eprintln!(
            "xtask: sdk-matrix — the recaptured matrix differs from the committed one. If an \
             SDK or the validator changed on purpose, re-record with `BLESS=1 cargo xtask \
             sdk-matrix --recaptured {}` and re-bless; otherwise the difference is the news",
            dir.display()
        );
    }
    ok
}

/// One cell of the run against its committed golden.
fn compare(root: &Path, dir: &Path, revision: &Revision, stem: &str) -> Result<String, String> {
    let golden = root.join(revision.goldens).join(format!("{stem}.json"));
    let recorded = dir.join(format!("{stem}.jsonl"));
    let declined = dir.join(format!("{stem}.{NOT_SERVED}"));
    match (recorded.is_file(), declined.is_file(), golden.is_file()) {
        (true, _, true) => {
            let fresh = judge(&recorded, revision.id)?;
            let committed: Value = serde_json::from_str(
                &fs::read_to_string(&golden)
                    .map_err(|error| format!("cannot read {}: {error}", golden.display()))?,
            )
            .map_err(|error| format!("{} is not JSON: {error}", golden.display()))?;
            let differences = differences(&without_seq(&committed), &without_seq(&fresh));
            if differences.is_empty() {
                Ok(format!("same result ({})", committed["verdict"]))
            } else {
                Err(format!(
                    "differs from the committed report: {}",
                    differences.join("; ")
                ))
            }
        }
        (true, _, false) => Err(format!(
            "recorded at {}, but the corpus holds no capture: the server serves the revision now",
            revision.id
        )),
        (false, true, true) => Err(format!(
            "the server no longer serves {}, but the corpus holds a capture of it",
            revision.id
        )),
        (false, true, false) => Ok("not served, as committed".to_owned()),
        (false, false, _) => Err(format!(
            "{} holds neither {stem}.jsonl nor {stem}.{NOT_SERVED}; did matrix.sh run?",
            dir.display()
        )),
    }
}

/// Copies the run's trace for one cell into the corpus, or removes the
/// committed one when the server stopped serving the revision.
fn adopt(root: &Path, dir: &Path, revision: &Revision, stem: &str) -> Result<String, String> {
    let recorded = dir.join(format!("{stem}.jsonl"));
    let committed = root.join(revision.traces).join(format!("{stem}.jsonl"));
    if recorded.is_file() {
        fs::create_dir_all(root.join(revision.traces))
            .and_then(|()| fs::copy(&recorded, &committed))
            .map_err(|error| format!("cannot copy into {}: {error}", committed.display()))?;
        return Ok(format!("adopted into {}", revision.traces));
    }
    if !dir.join(format!("{stem}.{NOT_SERVED}")).is_file() {
        return Err(format!("{} holds no recording of it", dir.display()));
    }
    let golden = root.join(revision.goldens).join(format!("{stem}.json"));
    for stale in [committed, golden] {
        if stale.is_file() {
            fs::remove_file(&stale)
                .map_err(|error| format!("cannot remove {}: {error}", stale.display()))?;
        }
    }
    Ok("not served; no capture kept".to_owned())
}

/// The trace's report as its golden pins it: judged against `revision`'s
/// registry, excluded rows left to the revision's ledger.
fn judge(trace: &Path, revision: &str) -> Result<Value, String> {
    let document = fs::read_to_string(trace)
        .map_err(|error| format!("cannot read {}: {error}", trace.display()))?;
    let events = mcp_trace_validator::reader::parse_trace(
        &document,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .map_err(|error| format!("{} is malformed: {error}", trace.display()))?;
    let set = RegistrySet::builtin().map_err(|error| format!("registry set: {error}"))?;
    let registry = revision
        .parse()
        .ok()
        .and_then(|revision| set.registry(revision))
        .ok_or_else(|| format!("this build does not describe {revision}"))?;
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    let mut value =
        serde_json::to_value(&report).map_err(|error| format!("cannot serialize: {error}"))?;
    if let Some(rows) = value["requirements"].as_array_mut() {
        rows.retain(|row| row["outcome"] != "excluded");
    }
    Ok(value)
}

/// `value` with every finding's `seq` removed.
fn without_seq(value: &Value) -> Value {
    match value {
        Value::Object(members) => Value::Object(
            members
                .iter()
                .filter(|(key, _)| key.as_str() != "seq")
                .map(|(key, member)| (key.clone(), without_seq(member)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_seq).collect()),
        other => other.clone(),
    }
}

/// What changed between two reports, named by requirement where it can be.
fn differences(committed: &Value, fresh: &Value) -> Vec<String> {
    let mut found = Vec::new();
    for key in ["revision", "verdict", "totals", "wire_schema"] {
        if committed[key] != fresh[key] {
            found.push(format!(
                "{key} was {} and is now {}",
                committed[key], fresh[key]
            ));
        }
    }
    let rows = |report: &Value| -> Vec<Value> {
        report["requirements"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    let (before, after) = (rows(committed), rows(fresh));
    for row in &before {
        let now = after.iter().find(|candidate| candidate["id"] == row["id"]);
        if now != Some(row) {
            found.push(format!(
                "{} was {} and is now {}",
                row["id"],
                row["outcome"],
                now.map_or_else(|| Value::from("absent"), |now| now["outcome"].clone())
            ));
        }
    }
    for row in &after {
        if !before.iter().any(|candidate| candidate["id"] == row["id"]) {
            found.push(format!("{} is new, {}", row["id"], row["outcome"]));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_finding_written_a_line_earlier_is_the_same_result() {
        let committed = json!({"verdict": "fail", "requirements": [
            {"id": "A-1", "outcome": "fail", "findings": [{"check": "c", "seq": 4, "detail": "d"}]}
        ]});
        let fresh = json!({"verdict": "fail", "requirements": [
            {"id": "A-1", "outcome": "fail", "findings": [{"check": "c", "seq": 5, "detail": "d"}]}
        ]});
        assert_eq!(
            differences(&without_seq(&committed), &without_seq(&fresh)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_moved_outcome_or_detail_is_named() {
        let committed = json!({"verdict": "pass", "requirements": [
            {"id": "A-1", "outcome": "pass"},
            {"id": "A-2", "outcome": "fail", "findings": [{"detail": "old"}]}
        ]});
        let fresh = json!({"verdict": "fail", "requirements": [
            {"id": "A-1", "outcome": "fail"},
            {"id": "A-2", "outcome": "fail", "findings": [{"detail": "new"}]},
            {"id": "A-3", "outcome": "pass"}
        ]});
        let found = differences(&committed, &fresh);
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found[0].starts_with("verdict was \"pass\""));
        assert!(found[1].contains("\"A-1\" was \"pass\" and is now \"fail\""));
        assert!(found[2].contains("\"A-2\""));
        assert!(found[3].contains("\"A-3\" is new"));
    }
}
