// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A baseline in SARIF: accepted findings carry SARIF's own `suppression`, the
//! entries that did not do their job are notifications, and without a baseline
//! none of it appears.

use serde_json::{Value, json};

use super::super::{Artifact, Options, render_run, render_run_with_baseline};
use crate::baseline::{Entry, Notes};
use crate::fixtures::{baselined_sdk_session, sdk_session};

fn log(judgment: &crate::judge::Judgment, notes: &Notes, options: Options) -> Value {
    let artifact = Artifact {
        uri: Some("t.jsonl"),
        events: &judgment.events,
    };
    serde_json::from_str(&render_run_with_baseline(
        &[(artifact, &judgment.reports[..])],
        &[],
        &options,
        notes,
    ))
    .unwrap()
}

fn results(log: &Value) -> &Vec<Value> {
    log["runs"][0]["results"].as_array().unwrap()
}

#[test]
fn an_accepted_finding_is_a_suppressed_note_with_its_reason_as_the_justification() {
    let known = baselined_sdk_session("the SDK's generator emits draft-07");
    let log = log(&known, &Notes::default(), Options::default());
    let found = results(&log);
    assert_eq!(found.len(), 2);
    for result in found {
        // SARIF 2.1.0 §3.35: where the decision is kept, its status, and why.
        assert_eq!(
            result["suppressions"],
            json!([{
                "kind": "external",
                "status": "accepted",
                "justification": "the SDK's generator emits draft-07",
            }])
        );
        assert_eq!(
            result["level"], "note",
            "a consumer ignoring suppressions still sees no error"
        );
        assert_eq!(result["properties"]["counted"], false);
        // The fingerprint is the baseline's key and is untouched by it.
        assert!(
            result["partialFingerprints"]["mcpConformanceFinding/v2"]
                .as_str()
                .unwrap()
                .starts_with("BASE-086:")
        );
    }
    assert_eq!(
        log["runs"][0]["properties"]["baseline"],
        json!({"accepted": 2, "unaccepted": 0, "unreviewed": 0})
    );
}

#[test]
fn an_expiry_and_an_unreviewed_reason_ride_in_the_suppressions_properties() {
    let mut known = baselined_sdk_session(crate::baseline::GENERATED_REASON);
    for row in &mut known.reports[0].requirements {
        for finding in &mut row.findings {
            if let Some(acceptance) = &mut finding.baselined {
                acceptance.expires = Some("2027-03-31".to_owned());
            }
        }
    }
    let log = log(&known, &Notes::default(), Options::default());
    assert_eq!(
        results(&log)[0]["suppressions"][0]["properties"],
        json!({"expires": "2027-03-31", "unreviewed": true})
    );
}

#[test]
fn a_finding_the_baseline_does_not_accept_is_an_ordinary_result_beside_the_suppressed_ones() {
    let mut judgment = baselined_sdk_session("known");
    let row = judgment.reports[0]
        .requirements
        .iter_mut()
        .find(|row| row.id == "BASE-086")
        .unwrap();
    let mut new = row.findings[0].clone();
    new.baselined = None;
    "the server sent tool \"d\"'s inputSchema (new)".clone_into(&mut new.detail);
    row.findings.push(new);
    let log = log(
        &judgment,
        &Notes::default(),
        Options::default().strict(true),
    );
    let found = results(&log);
    let new: Vec<&Value> = found
        .iter()
        .filter(|r| r.get("suppressions").is_none())
        .collect();
    assert_eq!(new.len(), 1);
    assert_eq!(new[0]["level"], "error", "strict, and not suppressed");
    assert_eq!(new[0]["properties"]["counted"], true);
    assert_eq!(
        found
            .iter()
            .filter(|r| r.get("suppressions").is_some())
            .count(),
        2
    );
}

#[test]
fn stale_and_lapsed_entries_are_notifications_and_do_not_fail_the_execution() {
    let known = baselined_sdk_session("known");
    let mut notes = Notes::default();
    notes
        .stale
        .push(Entry::new("BASE-086:check:gone:1", "was fixed"));
    notes.lapsed.push(
        Entry::new("BASE-086:check:old:1", "until July").expiring("2020-07-01".parse().unwrap()),
    );
    let log = log(&known, &notes, Options::default());
    let invocation = &log["runs"][0]["invocations"][0];
    let notifications = invocation["toolExecutionNotifications"].as_array().unwrap();
    let kinds: Vec<(&str, &str)> = notifications
        .iter()
        .map(|n| {
            (
                n["level"].as_str().unwrap(),
                n["properties"]["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [("warning", "baseline-stale"), ("note", "baseline-lapsed")]
    );
    assert_eq!(notifications[0]["properties"]["fails"], true);
    assert!(
        notifications[0]["message"]["text"]
            .as_str()
            .unwrap()
            .contains("exits 1")
    );
    assert_eq!(notifications[1]["properties"]["expires"], "2020-07-01");
    // Exit status 1 is a finding, not a failed execution.
    assert_eq!(invocation["executionSuccessful"], true);

    let allowed = super::super::render_run_with_baseline(
        &[(
            Artifact {
                uri: Some("t.jsonl"),
                events: &known.events,
            },
            &known.reports[..],
        )],
        &[],
        &Options::default(),
        &notes.clone().allowing_stale(true),
    );
    let allowed: Value = serde_json::from_str(&allowed).unwrap();
    let first = &allowed["runs"][0]["invocations"][0]["toolExecutionNotifications"][0];
    assert_eq!(first["properties"]["fails"], false);
    assert!(
        first["message"]["text"]
            .as_str()
            .unwrap()
            .contains("allowed")
    );
}

#[test]
fn without_a_baseline_nothing_of_one_appears() {
    let plain = sdk_session();
    let with_empty_notes = log(&plain, &Notes::default(), Options::default());
    assert!(
        results(&with_empty_notes)
            .iter()
            .all(|r| r.get("suppressions").is_none())
    );
    assert!(
        with_empty_notes["runs"][0]["properties"]
            .get("baseline")
            .is_none()
    );
    assert!(
        results(&with_empty_notes)
            .iter()
            .all(|r| r["properties"].get("counted").is_none())
    );
    let artifact = Artifact {
        uri: Some("t.jsonl"),
        events: &plain.events,
    };
    assert_eq!(
        render_run(&[(artifact, &plain.reports[..])], &[], &Options::default()),
        render_run_with_baseline(
            &[(artifact, &plain.reports[..])],
            &[],
            &Options::default(),
            &Notes::default()
        )
    );
}
