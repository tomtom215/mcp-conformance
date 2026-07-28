// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The committed side of `cargo xtask draft-readiness`: reading, writing, and
//! diffing `conformance/draft-readiness.json`.
//!
//! Split from the parent module purely for the 500-line file cap
//! (`docs/plan/04-engineering-standards.md`); the seam is a real one — the
//! parent measures, this decides whether the measurement is news.

use super::{BASELINE, DRAFT_SPEC_VERSION, DRAFT_SUITE_VERSION, FAILURE, INFO, SUCCESS, Scenario};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

/// The committed baseline: the pins it was measured under, plus every check's
/// status by scenario.
pub(super) fn read(path: &Path) -> Result<BTreeMap<String, Scenario>, String> {
    let body = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    let suite = parsed.get("suite").and_then(serde_json::Value::as_str);
    let spec = parsed.get("spec").and_then(serde_json::Value::as_str);
    if suite != Some(DRAFT_SUITE_VERSION) || spec != Some(DRAFT_SPEC_VERSION) {
        return Err(format!(
            "{} was measured under suite {suite:?} / spec {spec:?}, but this task pins \
             {DRAFT_SUITE_VERSION} / {DRAFT_SPEC_VERSION}. Re-measure with BLESS=1 \
             in the same commit that moves the pin.",
            path.display()
        ));
    }
    let scenarios = parsed
        .get("scenarios")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("{} has no `scenarios` object", path.display()))?;
    let mut baseline = BTreeMap::new();
    for (scenario, checks) in scenarios {
        let checks = checks
            .as_object()
            .ok_or_else(|| format!("{scenario} is not a map of check id to status"))?;
        let mut recorded = Scenario::new();
        for (check, status) in checks {
            let status = status
                .as_str()
                .ok_or_else(|| format!("{scenario} / {check} has a non-string status"))?;
            recorded.insert(check.clone(), status.to_owned());
        }
        baseline.insert(scenario.clone(), recorded);
    }
    Ok(baseline)
}

pub(super) fn write(path: &Path, measured: &BTreeMap<String, Scenario>) -> Result<(), String> {
    let tally = |wanted: &str| -> usize {
        measured
            .values()
            .flat_map(BTreeMap::values)
            .filter(|status| status.as_str() == wanted)
            .count()
    };
    let document = serde_json::json!({
        "_policy": "Measured by `cargo xtask draft-readiness`: the status of every check the \
                    official runner's 2026-07-28 scenarios report against the \
                    2025-11-25 everything server. The gate fails on ANY change — a lost pass \
                    is a migration regression, a gained one is progress that gets recorded \
                    deliberately (BLESS=1) in the commit that earned it. Statuses are verbatim: \
                    INFO is the runner's informational outcome and is neither a pass nor a \
                    failure. This is NOT a conformance claim about the 2026-07-28 revision: the \
                    text shipped on 2026-07-28 (register 1.5h), but the requirement registry \
                    does not describe that revision yet (roadmap M2.5 line 2), and these \
                    scenarios come from a pre-release suite, so the check set itself can \
                    still move.",
        "suite": DRAFT_SUITE_VERSION,
        "spec": DRAFT_SPEC_VERSION,
        "passing": tally(SUCCESS),
        "failing": tally(FAILURE),
        "informational": tally(INFO),
        "scenarios": measured,
    });
    let mut body = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("cannot render the baseline: {error}"))?;
    body.push('\n');
    std::fs::write(path, body).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Fails on any divergence, naming the direction so the reader knows whether
/// they broke something or finished something.
pub(super) fn compare(
    baseline: &BTreeMap<String, Scenario>,
    measured: &BTreeMap<String, Scenario>,
) -> ExitCode {
    let mut regressions = Vec::new();
    let mut changes = Vec::new();
    for (scenario, checks) in measured {
        let recorded = baseline.get(scenario);
        for (check, status) in checks {
            match recorded.and_then(|checks| checks.get(check)) {
                Some(was) if was == status => {}
                Some(was) if was == SUCCESS => {
                    regressions.push(format!("{scenario} / {check}: {was} -> {status}"));
                }
                Some(was) => changes.push(format!("{scenario} / {check}: {was} -> {status}")),
                None => changes.push(format!("{scenario} / {check}: new check, {status}")),
            }
        }
    }
    for (scenario, checks) in baseline {
        for (check, was) in checks {
            if !measured
                .get(scenario)
                .is_some_and(|checks| checks.contains_key(check))
            {
                let line = format!("{scenario} / {check}: gone from the suite (was {was})");
                if was == SUCCESS {
                    regressions.push(line);
                } else {
                    changes.push(line);
                }
            }
        }
    }
    if regressions.is_empty() && changes.is_empty() {
        eprintln!("xtask: draft-readiness — matches the committed baseline ({BASELINE})");
        return ExitCode::SUCCESS;
    }
    for line in &regressions {
        eprintln!("xtask: draft-readiness — REGRESSION {line}");
    }
    for line in &changes {
        eprintln!("xtask: draft-readiness — CHANGED     {line}");
    }
    eprintln!(
        "xtask: draft-readiness — the measurement no longer matches {BASELINE}. A regression \
         means the server lost ground against the next revision; any other change is \
         migration progress or suite drift. Either way it is recorded deliberately: re-run \
         with BLESS=1 and commit the new baseline alongside the change that caused it."
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::compare;
    use crate::draft_readiness::Scenario;
    use std::collections::BTreeMap;
    use std::process::ExitCode;

    fn measurement(rows: &[(&str, &[(&str, &str)])]) -> BTreeMap<String, Scenario> {
        rows.iter()
            .map(|(scenario, checks)| {
                let checks = checks
                    .iter()
                    .map(|(check, status)| ((*check).to_owned(), (*status).to_owned()))
                    .collect();
                ((*scenario).to_owned(), checks)
            })
            .collect()
    }

    #[test]
    fn an_identical_measurement_passes() {
        let recorded = measurement(&[
            ("tools-list", &[("tools-list", "FAILURE")]),
            (
                "dns-rebinding-protection",
                &[
                    ("rejects-evil-host", "SUCCESS"),
                    ("accepts-loopback", "FAILURE"),
                ],
            ),
        ]);
        assert_eq!(compare(&recorded, &recorded.clone()), ExitCode::SUCCESS);
    }

    #[test]
    fn losing_a_pass_is_a_regression() {
        let baseline = measurement(&[("dns", &[("rejects-evil-host", "SUCCESS")])]);
        let measured = measurement(&[("dns", &[("rejects-evil-host", "FAILURE")])]);
        assert_eq!(compare(&baseline, &measured), ExitCode::FAILURE);
    }

    #[test]
    fn unrecorded_progress_also_fails() {
        let baseline = measurement(&[("tools-list", &[("tools-list", "FAILURE")])]);
        let measured = measurement(&[("tools-list", &[("tools-list", "SUCCESS")])]);
        assert_eq!(compare(&baseline, &measured), ExitCode::FAILURE);
    }

    #[test]
    fn info_is_neither_a_pass_nor_a_failure() {
        // An INFO check flipping to FAILURE is a change, not a regression —
        // but it still fails the gate, so it cannot pass unnoticed.
        let baseline = measurement(&[("sse", &[("streams-functional", "INFO")])]);
        let measured = measurement(&[("sse", &[("streams-functional", "FAILURE")])]);
        assert_eq!(compare(&baseline, &measured), ExitCode::FAILURE);
    }

    #[test]
    fn a_check_the_suite_added_or_dropped_fails() {
        let baseline = measurement(&[("tools-list", &[("tools-list", "FAILURE")])]);
        assert_eq!(
            compare(
                &baseline,
                &measurement(&[(
                    "tools-list",
                    &[("tools-list", "FAILURE"), ("new", "FAILURE")]
                )])
            ),
            ExitCode::FAILURE
        );
        assert_eq!(compare(&baseline, &measurement(&[])), ExitCode::FAILURE);
    }
}
