// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The coverage-manifest check: what server surface the tapped suite sessions
//! prove, reconciled against the committed manifest
//! (`conformance/coverage-manifest.json`). `BLESS=1` regenerates, like every
//! other golden artifact in this repository.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mcp_conformance_core::capability::CapabilityParty;
use mcp_conformance_core::requirement::Registry;
use mcp_conformance_core::trace::TraceEvent;
use serde::Serialize;

/// Committed manifest of the server surface the suite exercised, relative to
/// the workspace root.
const MANIFEST_PATH: &str = "conformance/coverage-manifest.json";

/// The committed manifest: what surface the tapped suite sessions prove.
#[derive(Debug, Serialize)]
struct CoverageManifest {
    /// How to regenerate (kept in the artifact so it explains itself).
    #[serde(rename = "_generated")]
    generated: String,
    /// The registry revision the gates come from.
    spec_revision: String,
    /// The server's declared capabilities, from the initialize result.
    server_capabilities: serde_json::Value,
    /// Every server-party capability gate in the registry, with whether the
    /// server declared it. All must be true: an undeclared gate means a slice
    /// of the registry silently became not-applicable.
    capability_gates: BTreeMap<String, bool>,
    /// Request methods the suite drove, as observed on the wire.
    methods_observed: BTreeSet<String>,
}

/// Builds the manifest from the tapped sessions and checks it against the
/// committed copy (or rewrites the committed copy under `BLESS=1`).
pub(crate) fn check_manifest(
    workspace_root: &Path,
    registry: &Registry,
    sessions: &[(String, Vec<TraceEvent>)],
) -> Result<(), String> {
    let server_capabilities = sessions
        .iter()
        .find_map(|(_, events)| initialize_capabilities(events))
        .ok_or_else(|| {
            "no initialize result with capabilities found in any tapped session".to_owned()
        })?;

    let capability_gates = server_gates(registry, &server_capabilities);
    let methods_observed = observed_methods(sessions);

    let manifest = CoverageManifest {
        generated: "cargo xtask conformance (BLESS=1 to regenerate)".to_owned(),
        // The registry's revision, not a literal: when M2.5 adds a second
        // revision, the manifest must follow the registry under test.
        spec_revision: registry.revision().to_string(),
        server_capabilities,
        capability_gates: capability_gates.clone(),
        methods_observed,
    };

    if let Some((gate, _)) = capability_gates.iter().find(|(_, declared)| !**declared) {
        return Err(format!(
            "registry gate {gate} is not declared by the server — a slice of \
             the registry silently became not-applicable; declare the \
             capability or document the exclusion"
        ));
    }

    let rendered = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("manifest unserializable: {error}"))?
        + "\n";
    let path = workspace_root.join(MANIFEST_PATH);
    if std::env::var_os("BLESS").is_some_and(|v| v == "1") {
        std::fs::write(&path, &rendered)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        eprintln!("xtask: agreement — blessed {}", path.display());
        return Ok(());
    }
    let committed = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read {}: {error} (BLESS=1 to create it)",
            path.display()
        )
    })?;
    if committed == rendered {
        eprintln!(
            "xtask: agreement — coverage manifest in sync ({})",
            path.display()
        );
        Ok(())
    } else {
        Err(format!(
            "{} is out of sync with the tapped sessions — review the change \
             and regenerate with BLESS=1 cargo xtask conformance",
            path.display()
        ))
    }
}

/// Every server-party capability gate in the registry, with whether the
/// server's declared capabilities satisfy it.
fn server_gates(
    registry: &Registry,
    server_capabilities: &serde_json::Value,
) -> BTreeMap<String, bool> {
    let mut gates = BTreeMap::new();
    for requirement in registry.requirements() {
        if let Some(gate) = &requirement.capability
            && gate.party() == CapabilityParty::Server
        {
            gates.insert(
                gate.as_str().to_owned(),
                gate.is_declared(Some(server_capabilities)),
            );
        }
    }
    gates
}

/// Every request method observed across the tapped sessions.
fn observed_methods(sessions: &[(String, Vec<TraceEvent>)]) -> BTreeSet<String> {
    let mut methods = BTreeSet::new();
    for (_, events) in sessions {
        for event in events {
            if let Some(method) = event
                .message_payload()
                .and_then(|payload| payload.get("method"))
                .and_then(serde_json::Value::as_str)
            {
                methods.insert(method.to_owned());
            }
        }
    }
    methods
}

/// The `capabilities` object from the first initialize *result* in a session.
fn initialize_capabilities(events: &[TraceEvent]) -> Option<serde_json::Value> {
    // The initialize request's id, so the matching result can be found.
    let init_id = events.iter().find_map(|event| {
        let payload = event.message_payload()?;
        (payload.get("method")? == "initialize").then(|| payload.get("id").cloned())?
    })?;
    events.iter().find_map(|event| {
        let payload = event.message_payload()?;
        if payload.get("id") == Some(&init_id) {
            payload.get("result")?.get("capabilities").cloned()
        } else {
            None
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mcp_trace_validator::reader::{Limits, parse_trace};

    #[test]
    fn initialize_capabilities_pairs_request_id_with_result() {
        let trace = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}}
{"seq":1,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":7,"result":{"capabilities":{"tools":{}}}}}"#;
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let caps = initialize_capabilities(&events).unwrap();
        assert_eq!(caps, serde_json::json!({"tools": {}}));
    }
}
