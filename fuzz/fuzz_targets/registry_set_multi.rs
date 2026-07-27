// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `--registry-set` is untrusted input, and multi-revision judgment is the only
//! engine path whose *shape* is attacker-influenced rather than fixed: the
//! caller's document decides how many revisions exist, which requirements apply
//! to which of them, and therefore how the per-clause rows are aligned.
//!
//! `registry_parse` already covers the single-revision loader, but it cannot
//! reach any of that — projection to a revision, the applies-range filter, or
//! the alignment that has to keep "absent at this revision" distinct from
//! "not applicable here" (ADR-0006). This target drives the whole path: parse a
//! set from arbitrary bytes, project it to every revision it claims to
//! describe, and run real judgment across all of them at once. Parsing may
//! fail and judgment may return findings; neither may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mcp_conformance_core::requirement::RegistrySet;
use mcp_trace_validator::multi;
use mcp_trace_validator::reader::{self, Limits};

/// A minimal well-formed session, so judgment runs against real events rather
/// than an empty slice. The interesting variable here is the *registry set*,
/// not the trace — `trace_parse` fuzzes the other side.
const SESSION: &str = concat!(
    r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"f","version":"1"}}}}"#,
    "\n",
    r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"f","version":"1"}}}}"#,
    "\n",
    r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
    "\n",
);

fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };
    let Ok(set) = RegistrySet::from_json(text) else {
        // Malformed or invalid sets are the common case and are rejected by
        // contract; the assertion is only that rejection is not a panic.
        return;
    };

    // Projection must be total over the revisions the set itself declares: a
    // set that validates but cannot produce a registry for a revision it names
    // would be an internal contradiction rather than bad input.
    let revisions = set.revisions().to_vec();
    for &revision in &revisions {
        assert!(
            set.registry(revision).is_some(),
            "a validated set failed to project to a revision it declares"
        );
    }

    // Judge one real session against every declared revision at once — the
    // alignment path, including the absent-versus-not-applicable distinction.
    let events = reader::parse_trace(SESSION, &Limits::default())
        .expect("the embedded session is well-formed");
    if let Ok(report) = multi::validate_revisions(&set, &revisions, &events) {
        // The report's own shape must describe what it actually compared: one
        // column per requested revision, on every row and in the summaries.
        assert_eq!(report.revisions.len(), revisions.len());
        assert_eq!(report.summaries.len(), revisions.len());
        for row in &report.requirements {
            assert_eq!(
                row.outcomes.len(),
                revisions.len(),
                "a report row disagrees with the number of revisions judged"
            );
        }
    }
});
