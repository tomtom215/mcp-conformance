// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reading the runner's side: its `wire-schema-valid` and
//! `wire-schema-harness-error` checks, as `0.2.0-alpha.11` and later write
//! them into each scenario's `checks.json` (the parent module's docs say
//! what they mean).

use std::path::Path;

use serde_json::Value;

use super::{Flagged, Origin, SUITE_CHECK, SUITE_HARNESS_CHECK, SuiteSide, canonical};

/// Reads the runner's wire-schema checks from every `<scenario>/checks.json`
/// under `results_dir`. A shape the module does not recognise is an error,
/// never a silent pass.
pub(crate) fn read_suite(results_dir: &Path) -> Result<SuiteSide, String> {
    let mut side = SuiteSide::default();
    let entries = std::fs::read_dir(results_dir)
        .map_err(|error| format!("cannot read {}: {error}", results_dir.display()))?;
    let mut dirs: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let path = dir.join("checks.json");
        if !path.is_file() {
            continue;
        }
        let scenario = dir
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let checks: Vec<Value> = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid: {error}", path.display()))?;
        read_checks(&mut side, &scenario, &checks)
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    side.flagged_messages = side.flagged.len();
    Ok(side)
}

/// Folds one scenario's checks into `side`.
pub(crate) fn read_checks(
    side: &mut SuiteSide,
    scenario: &str,
    checks: &[Value],
) -> Result<(), String> {
    for check in checks {
        let id = check.get("id").and_then(Value::as_str).unwrap_or_default();
        if id != SUITE_CHECK && id != SUITE_HARNESS_CHECK {
            continue;
        }
        let status = check
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let violations = check
            .pointer("/details/violations")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("`{id}` carries no details.violations array"))?;
        if (status == "FAILURE") == violations.is_empty() {
            return Err(format!(
                "`{id}` is {status:?} with {} violation(s); the module reads \
                 FAILURE as exactly \"some violation\"",
                violations.len()
            ));
        }
        if id == SUITE_CHECK {
            side.scenarios += 1;
            side.failing += usize::from(status == "FAILURE");
            side.messages_validated += check
                .pointer("/details/messagesValidated")
                .and_then(Value::as_u64)
                .unwrap_or(0);
        }
        for violation in violations {
            side.flagged.push(read_violation(scenario, violation)?);
        }
    }
    Ok(())
}

fn read_violation(scenario: &str, violation: &Value) -> Result<Flagged, String> {
    let origin = match violation.get("origin").and_then(Value::as_str) {
        Some("implementation") => Origin::Implementation,
        Some("harness") => Origin::Harness,
        other => return Err(format!("a violation has origin {other:?}")),
    };
    let message = violation
        .get("message")
        .ok_or("a violation carries no message")?;
    let errors = violation
        .get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .map(|error| error.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default();
    Ok(Flagged {
        scenario: scenario.to_owned(),
        origin,
        key: canonical(message),
        errors,
    })
}
