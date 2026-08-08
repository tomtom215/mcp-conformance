// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `2026-07-28` error-code partition.
//!
//! The partition is four checks reading six constants, and a corpus trace only
//! ever proves that *one* code lands in the right bucket. What matters here is
//! the shape of each bucket: the codes just outside a range must stay outside,
//! and a code the specification defines must not be reported as undefined. Each
//! assertion below pins one edge.

use crate::checks::draft::testkit::{client, error, findings_for, trace};

/// What `check` reports for a session whose single error carries `code`.
fn for_code(check: &str, code: i64) -> Vec<String> {
    let document = trace(&[
        client(0, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#),
        error(1, "1", code),
    ]);
    findings_for(check, &document)
}

/// Asserts exactly which of `codes` the check reports.
fn assert_reports(check: &str, reported: &[i64], ignored: &[i64]) {
    for code in reported {
        assert_eq!(
            for_code(check, *code).len(),
            1,
            "{check} should report {code}"
        );
    }
    for code in ignored {
        assert!(
            for_code(check, *code).is_empty(),
            "{check} should ignore {code}"
        );
    }
}

#[test]
fn the_legacy_sub_range_is_closed_at_both_ends() {
    // -32000..-32019 inclusive, and nothing else: the neighbours on each side
    // belong to the MCP-reserved sub-range and to the application space.
    assert_reports(
        "base.error-code-legacy-subrange",
        &[-32000, -32010, -32019],
        &[-32020, -31999, -32700, -1],
    );
}

#[test]
fn the_reserved_sub_range_reports_only_codes_the_specification_leaves_undefined() {
    // Inside -32020..-32099, the three codes this revision defines are legal and
    // the rest are not; outside it, nothing is this check's business.
    assert_reports(
        "base.error-code-reserved-subrange",
        &[-32055, -32099, -32023],
        &[-32020, -32021, -32022, -32010, -32100, -32700],
    );
}

#[test]
fn withdrawn_codes_are_named_exactly() {
    // Both withdrawn codes, and no code that merely sits near them.
    assert_reports(
        "base.error-code-withdrawn",
        &[-32002, -32042],
        &[-32001, -32003, -32041, -32043, -32601],
    );
}

#[test]
fn the_application_range_check_fires_only_on_what_nothing_else_accounts_for() {
    // Inside the JSON-RPC reserved range and not standard, not MCP-defined, not
    // in either sub-range — that remainder is the clause's subject.
    assert_reports(
        "base.error-code-application-range",
        &[-32500, -32768, -32200],
        &[
            // Standard JSON-RPC codes stay legal.
            -32700, -32600, -32601, -32602, -32603,
            // Codes this revision defines, and the two sub-ranges its siblings own.
            -32020, -32021, -32022, -32010, -32055,
            // Outside the reserved range entirely: application space, as intended.
            -1, 0, -31999, -40000,
        ],
    );
}

#[test]
fn a_non_integer_code_is_left_to_its_own_clause() {
    // BASE-054 owns "the code must be an integer"; reporting it here too would
    // blame the partition for a defect that is not about the partition.
    let document = trace(&[
        client(0, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#),
        crate::checks::draft::testkit::server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":"-32010","message":"x"}}"#,
        ),
    ]);
    for check in [
        "base.error-code-legacy-subrange",
        "base.error-code-reserved-subrange",
        "base.error-code-withdrawn",
        "base.error-code-application-range",
    ] {
        assert!(findings_for(check, &document).is_empty(), "{check}");
    }
}
