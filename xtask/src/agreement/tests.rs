// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The gate around the shared policy. The policy's own proofs (explaining by
//! requirement and trace filter, staleness, rejected baselines) live with it,
//! in `mcp_conformance::agreement`; these pin what this module adds: reading
//! the baseline from disk, and the artifact's shape.

#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn a_baseline_error_names_the_file() {
    let dir = std::env::temp_dir().join(format!("agreement-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("baseline.json");

    let missing = load_baseline(&path).unwrap_err();
    assert!(missing.starts_with("cannot read "), "{missing}");
    assert!(missing.contains("baseline.json"), "{missing}");

    std::fs::write(
        &path,
        r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"wontfix","upstream":"https://e.example"}]}"#,
    )
    .unwrap();
    let invalid = load_baseline(&path).unwrap_err();
    assert!(invalid.contains("baseline.json: "), "{invalid}");
    assert!(invalid.contains("class"), "{invalid}");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_committed_baselines_load() {
    let root = crate::workspace_root();
    load_baseline(&root.join(DIVERGENCE_BASELINE)).unwrap();
    load_baseline(&root.join("conformance/client-agreement-divergences.json")).unwrap();
}

/// A wire-schema reconciliation of a run with nothing to compare.
fn no_schema_divergences() -> schema_ledger::SchemaAgreement {
    schema_ledger::reconcile(
        &schema::SERVER_LEG,
        &schema_ledger::parse(r#"{"policy":"p","divergences":[]}"#).unwrap(),
        schema::SuiteSide::default(),
        schema::Comparison {
            sessions: Vec::new(),
            divergences: Vec::new(),
            unjudged: Vec::new(),
        },
    )
}

/// `target/conformance/agreement.json` keeps the shape it had before the
/// policy moved into the library, beside the wire-schema reconciliation.
#[test]
fn the_artifact_keeps_its_shape() {
    let failure = ValidatorFailure::new("030-x.jsonl", "LIFE-003", "no initialized");
    let baseline = Baseline::parse(
        r#"{"policy":"p","divergences":[{"requirement":"LIFE-003","class":"suite-bug","upstream":"https://e.example"}]}"#,
    )
    .unwrap();
    let reconciliation = agreement::reconcile(&baseline, std::slice::from_ref(&failure));
    let artifact = AgreementArtifact {
        suite_version: "0.1.16".to_owned(),
        spec_revision: "2025-11-25".to_owned(),
        runner: Summary::default(),
        validator: ValidatorSide {
            sessions: 1,
            totals: Totals::default(),
            failures: vec![failure],
        },
        agreement: reconciliation.holds(),
        divergences: reconciliation,
        wire_schema: no_schema_divergences(),
    };
    let json = serde_json::to_value(&artifact).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "agreement",
            "divergences",
            "runner",
            "spec_revision",
            "suite_version",
            "validator",
            "wire_schema"
        ]
    );
    assert_eq!(
        json["divergences"]["explained"][0]["requirement"],
        "LIFE-003"
    );
    assert_eq!(
        json["divergences"]["stale_baseline_entries"],
        serde_json::json!([])
    );
    assert_eq!(
        json["runner"],
        serde_json::json!({"scenarios": 0, "checks": 0, "checks_by_status": {}})
    );
    assert_eq!(json["validator"]["totals"]["not_observed"], 0);
    assert_eq!(json["agreement"], true);
}
