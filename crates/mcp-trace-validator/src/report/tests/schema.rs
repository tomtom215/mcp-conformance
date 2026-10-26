// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The wire schema's line and row in the human report, and an extension's own
//! verdict.

use super::super::*;

/// A wire-schema result with `outcome`, three messages, and one finding when
/// it failed.
fn schema(outcome: Outcome) -> WireSchemaReport {
    let findings = if outcome == Outcome::Fail {
        vec![Finding {
            check: "wire-schema".to_owned(),
            seq: Some(4),
            party: Some(Party::Client),
            detail: "not a valid JSONRPCRequest".to_owned(),
        }]
    } else {
        vec![]
    };
    WireSchemaReport {
        outcome,
        messages: if outcome == Outcome::NotObserved {
            0
        } else {
            3
        },
        findings,
        schema: "https://example.test/schema.json".to_owned(),
        source: ClauseSource {
            section: "basic#schema".to_owned(),
            quote: "the source of truth".to_owned(),
            url: "https://example.test/basic#schema".to_owned(),
        },
    }
}

fn row_of(outcome: Outcome, all: bool) -> String {
    let mut out = String::new();
    schema(outcome).write_human(&mut out, &HumanOptions::default().all(all), None, None);
    out
}

#[test]
fn each_schema_outcome_has_its_own_summary() {
    assert_eq!(
        schema(Outcome::NotObserved).summary(),
        "not observed (the trace holds no message)"
    );
    assert_eq!(
        schema(Outcome::Fail).summary(),
        "fail (1 of 3 messages invalid)"
    );
    assert_eq!(schema(Outcome::Pass).summary(), "pass (3 messages valid)");
}

#[test]
fn a_failed_schema_row_names_its_finding_clause_and_schema() {
    let expected = "  FAIL  wire schema — fail (1 of 3 messages invalid)\n        \
                    [client] seq 4: not a valid JSONRPCRequest\n        \
                    spec: \"the source of truth\"\n        \
                    see:  https://example.test/basic#schema\n        \
                    schema: https://example.test/schema.json\n";
    assert_eq!(row_of(Outcome::Fail, false), expected);
    assert_eq!(row_of(Outcome::Fail, true), expected);
}

#[test]
fn a_schema_row_that_did_not_fail_shows_only_when_every_row_is_listed() {
    assert_eq!(row_of(Outcome::Pass, false), "");
    assert_eq!(row_of(Outcome::NotObserved, false), "");
    assert_eq!(
        row_of(Outcome::Pass, true),
        "  PASS  wire schema — pass (3 messages valid)\n"
    );
    assert_eq!(
        row_of(Outcome::NotObserved, true),
        "  NOBS  wire schema — not observed (the trace holds no message)\n"
    );
}

#[test]
fn an_extension_verdict_ranks_unsupported_first() {
    let extension = |totals: Totals| ExtensionReport {
        extension: "io.example/x".to_owned(),
        totals,
        requirements: vec![],
    };
    let unsupported = Totals {
        unsupported: 1,
        fail: 1,
        warn: 1,
        pass: 1,
        ..Totals::default()
    };
    assert_eq!(extension(unsupported).verdict(), Verdict::Unsupported);
    let failed = Totals {
        unsupported: 0,
        ..unsupported
    };
    assert_eq!(extension(failed).verdict(), Verdict::Fail);
    assert_eq!(
        extension(Totals { fail: 0, ..failed }).verdict(),
        Verdict::PassWithWarnings
    );
    assert_eq!(
        extension(Totals {
            pass: 1,
            ..Totals::default()
        })
        .verdict(),
        Verdict::Pass
    );
}
