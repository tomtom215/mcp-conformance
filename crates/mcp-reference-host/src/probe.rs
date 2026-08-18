// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The probe session: deliberately malformed requests, to record what a server
//! does when it must refuse (feature `http`).
//!
//! Every other capture in this workspace is a *conforming* client, and a
//! conforming client can never exercise a rejection rule. Fifteen `2026-07-28`
//! clauses say what a server owes a request it must not serve — a `_meta`
//! envelope missing a required field, a protocol version the server does not
//! implement, a header disagreeing with the body it mirrors, a log level
//! outside RFC 5424's eight, a cursor nobody issued — and every one of them
//! reported *not observed* on every recording, because nothing had ever sent
//! such a request.
//!
//! **This module sends them, and it does so outside rmcp on purpose.** rmcp's
//! client is the thing that makes the other captures trustworthy: it builds the
//! `_meta` envelope, mirrors the SEP-2243 headers, and refuses to emit an
//! ill-formed request. That is exactly why it cannot be the probe. So the
//! probes are hand-built HTTP requests — the bytes are the fixture — and each
//! one names the clause it exists to put a question to.
//!
//! Nothing here asserts what the server *should* answer. The probe records; the
//! registry judges the recording. A probe that carried its own expectations
//! would be a second, weaker implementation of the checks, and the two would
//! drift.

use std::time::Duration;

/// The protocol revision every probe names, except where naming another is the
/// point.
const REVISION: &str = "2026-07-28";

/// A well-formed `_meta` envelope, as the required-field probes' control.
const ENVELOPE: &str = r#""io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}"#;

/// How long one probe may take. Generous: a rejection is cheap to compute, and
/// the whole session is a dozen round trips against a loopback server.
const TIMEOUT: Duration = Duration::from_secs(10);

/// One deliberately malformed request, and the clause it questions.
#[derive(Debug, Clone)]
pub struct Probe {
    /// The clause ids this probe exists to put traffic under, for the run
    /// record. Nothing reads them to decide anything — the registry judges the
    /// recording — but an operator reading a capture needs to know why a
    /// malformed request is in it.
    pub clauses: &'static str,
    /// What is wrong with the request, in the words a finding would use.
    pub fault: &'static str,
    /// Headers to send, over the `content-type`/`accept` pair every probe
    /// carries. A probe testing header rules overrides them here.
    pub headers: Vec<(&'static str, String)>,
    /// The exact JSON-RPC body.
    pub body: String,
}

/// What one probe drew.
#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    /// The clause ids, echoed for the run record.
    pub clauses: &'static str,
    /// The fault, echoed for the run record.
    pub fault: &'static str,
    /// The HTTP status, or the transport error that prevented one.
    pub answer: Result<u16, String>,
}

/// Every probe, in the order they are sent.
///
/// Ordered so the version-negotiation pair reads as a story: the request naming
/// an unsupported version comes first, and the one naming a supported version
/// follows it, which is what `VERS-002` binds — a client told which versions a
/// server implements must use one of them afterwards.
#[must_use]
pub fn session() -> Vec<Probe> {
    let mut probes = version_probes();
    probes.extend(envelope_probes());
    probes.push(legacy_initialize());
    probes
}

/// The version-negotiation story, in the order that makes it one.
///
/// The refusal first, the retry after it: `VERS-002` binds what a client does
/// *after* being told which versions a server implements, and a retry recorded
/// before the refusal would test nothing.
fn version_probes() -> Vec<Probe> {
    vec![
        probe(
            "BASE-031, BASE-032",
            "`_meta` omits the required clientCapabilities",
            "ping",
            None,
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"{REVISION}"}}}}}}"#
            ),
        ),
        probe(
            "TRAN-074, TRAN-102, VERS-001",
            "names a protocol version this server does not implement",
            "ping",
            Some("1999-01-01"),
            r#"{"jsonrpc":"2.0","id":2,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1999-01-01","io.modelcontextprotocol/clientCapabilities":{}}}}"#
                .to_owned(),
        ),
        probe(
            "VERS-002",
            "the retry, naming a version the refusal said was supported",
            "tools/list",
            None,
            format!(
                r#"{{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{{"_meta":{{{ENVELOPE}}}}}}}"#
            ),
        ),
        probe(
            "TRAN-073, TRAN-098",
            "MCP-Protocol-Version header disagrees with the body's `_meta`",
            "ping",
            Some("2025-11-25"),
            format!(
                r#"{{"jsonrpc":"2.0","id":4,"method":"ping","params":{{"_meta":{{{ENVELOPE}}}}}}}"#
            ),
        ),
    ]
}

/// The probes whose fault is in the request's own content rather than its
/// version: an unknown method, a bad log level, a fabricated cursor, and a
/// capability the request never declared.
fn envelope_probes() -> Vec<Probe> {
    let mut probes = vec![
        probe(
            "TRAN-075",
            "asks for a method no server implements",
            "no/such/method",
            None,
            format!(
                r#"{{"jsonrpc":"2.0","id":5,"method":"no/such/method","params":{{"_meta":{{{ENVELOPE}}}}}}}"#
            ),
        ),
        probe(
            "LOG-010",
            "asks for a log level outside RFC 5424's eight",
            "tools/list",
            None,
            format!(
                r#"{{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{{"_meta":{{{ENVELOPE},"io.modelcontextprotocol/logLevel":"chatty"}}}}}}"#
            ),
        ),
        probe(
            "PAGE-011",
            "presents a cursor this server never issued",
            "tools/list",
            None,
            format!(
                r#"{{"jsonrpc":"2.0","id":7,"method":"tools/list","params":{{"_meta":{{{ENVELOPE}}},"cursor":"fabricated-by-the-probe"}}}}"#
            ),
        ),
        probe(
            "BASE-035, BASE-036, MRTR-012",
            "calls a tool needing sampling while declaring no capabilities",
            "tools/call",
            None,
            format!(
                r#"{{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{{"_meta":{{{ENVELOPE}}},"name":"test_sampling","arguments":{{"prompt":"probe"}}}}}}"#
            ),
        ),
    ];
    // `tools/call` mirrors its `params.name` into `Mcp-Name` (SEP-2243), and a
    // POST without it is rejected for the *header* rather than reaching the
    // capability check this probe is about.
    if let Some(call) = probes.last_mut() {
        call.headers.push(("mcp-name", "test_sampling".to_owned()));
    }
    probes
}

/// One probe, with the two headers every POST carries plus its own.
fn probe(
    clauses: &'static str,
    fault: &'static str,
    method: &'static str,
    version: Option<&str>,
    body: String,
) -> Probe {
    Probe {
        clauses,
        fault,
        headers: vec![
            ("mcp-method", method.to_owned()),
            (
                "mcp-protocol-version",
                version.unwrap_or(REVISION).to_owned(),
            ),
        ],
        body,
    }
}

/// The legacy handshake, which a `2026-07-28` server must refuse.
///
/// Sent with no `MCP-Protocol-Version` header at all, because that is what a
/// `2025-11-25` client is: the header is this revision's invention, and a probe
/// that sent one would be testing a client that does not exist.
fn legacy_initialize() -> Probe {
    Probe {
        clauses: "VERS-008",
        fault: "the removed handshake, from a client that speaks only the previous era",
        headers: vec![("mcp-method", "initialize".to_owned())],
        body: r#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}"#
            .to_owned(),
    }
}

/// Sends every probe to `url` in order, recording what each drew.
///
/// A probe that fails at the transport level is recorded and the session
/// continues: one refused connection must not cost the evidence for the rest.
pub async fn run(url: &str) -> Vec<ProbeOutcome> {
    let client = reqwest::Client::new();
    let mut outcomes = Vec::new();
    for probe in session() {
        let mut request = client
            .post(url)
            .timeout(TIMEOUT)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (name, value) in &probe.headers {
            request = request.header(*name, value);
        }
        let answer = request
            .body(probe.body)
            .send()
            .await
            .map(|response| response.status().as_u16())
            .map_err(|error| error.to_string());
        outcomes.push(ProbeOutcome {
            clauses: probe.clauses,
            fault: probe.fault,
            answer,
        });
    }
    outcomes
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn every_probe_is_a_well_formed_request_that_is_wrong_on_purpose() {
        // The bytes are the fixture, so a typo in one is a probe that tests
        // nothing and reports a pass. Each body must parse, carry the JSON-RPC
        // envelope and an id, and name a method.
        let probes = session();
        assert!(probes.len() >= 9, "{}", probes.len());
        let mut ids = std::collections::BTreeSet::new();
        for probe in &probes {
            let body: serde_json::Value = serde_json::from_str(&probe.body)
                .unwrap_or_else(|error| panic!("{}: {error}", probe.clauses));
            assert_eq!(body["jsonrpc"], "2.0", "{}", probe.clauses);
            assert!(body["method"].is_string(), "{}", probe.clauses);
            let id = body["id"].as_u64().expect("every probe is a request");
            assert!(ids.insert(id), "{}: id {id} is reused", probe.clauses);
            assert!(!probe.fault.is_empty());
        }
    }

    #[test]
    fn the_headers_a_probe_needs_are_the_ones_it_carries() {
        let probes = session();
        // `tools/call` mirrors `params.name` into `Mcp-Name`; without it the
        // POST is refused for the header and never reaches the clause under
        // test.
        let call = probes
            .iter()
            .find(|probe| probe.body.contains(r#""method":"tools/call""#))
            .expect("the capability probe calls a tool");
        assert!(
            call.headers
                .iter()
                .any(|(name, value)| *name == "mcp-name" && value == "test_sampling"),
            "{:?}",
            call.headers
        );
        // The legacy handshake carries no protocol-version header, because a
        // client of the previous era has none to send.
        let legacy = probes
            .iter()
            .find(|probe| probe.clauses == "VERS-008")
            .expect("the legacy handshake is probed");
        assert!(
            !legacy
                .headers
                .iter()
                .any(|(name, _)| *name == "mcp-protocol-version"),
            "{:?}",
            legacy.headers
        );
    }

    #[test]
    fn the_version_probes_read_as_a_negotiation() {
        // VERS-002 binds what a client does *after* being told which versions
        // a server supports, so the refusal must precede the retry.
        let probes = session();
        let refused = probes
            .iter()
            .position(|probe| probe.clauses.contains("VERS-001"))
            .expect("a version is refused");
        let retried = probes
            .iter()
            .position(|probe| probe.clauses == "VERS-002")
            .expect("and then retried");
        assert!(refused < retried, "{refused} !< {retried}");
    }
}
