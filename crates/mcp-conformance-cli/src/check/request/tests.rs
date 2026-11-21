// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use serde_json::{Map, Value, json};

use super::{Era, LEGACY, MODERN, Requests};

fn params(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[test]
fn a_modern_request_carries_the_envelope_and_the_metadata_headers() {
    let mut requests = Requests::new(Era::Modern);
    let out = requests.request(
        "tools/call",
        params(&[("name", json!("café")), ("arguments", json!({}))]),
        vec![("Mcp-Param-Region".to_owned(), "eu".to_owned())],
    );
    assert_eq!(out.id, Some(1));
    assert_eq!(out.body["id"], 1);
    assert_eq!(out.body["jsonrpc"], "2.0");
    let meta = &out.body["params"]["_meta"];
    assert_eq!(meta["io.modelcontextprotocol/protocolVersion"], MODERN);
    assert_eq!(
        meta["io.modelcontextprotocol/clientCapabilities"],
        json!({})
    );
    assert_eq!(
        meta["io.modelcontextprotocol/clientInfo"]["name"],
        "mcp-conformance-check"
    );
    assert_eq!(header(&out.headers, "MCP-Protocol-Version"), Some(MODERN));
    assert_eq!(header(&out.headers, "Mcp-Method"), Some("tools/call"));
    assert_eq!(
        header(&out.headers, "Mcp-Name"),
        Some("=?base64?Y2Fmw6k=?=")
    );
    assert_eq!(header(&out.headers, "Mcp-Param-Region"), Some("eu"));
}

#[test]
fn mcp_name_mirrors_the_name_or_the_uri_and_only_where_one_is_required() {
    let mut requests = Requests::new(Era::Modern);
    let read = requests.request(
        "resources/read",
        params(&[("uri", json!("file:///a"))]),
        Vec::new(),
    );
    assert_eq!(header(&read.headers, "Mcp-Name"), Some("file:///a"));
    let get = requests.request("prompts/get", params(&[("name", json!("p"))]), Vec::new());
    assert_eq!(header(&get.headers, "Mcp-Name"), Some("p"));
    let list = requests.request("tools/list", params(&[("cursor", json!("c"))]), Vec::new());
    assert_eq!(header(&list.headers, "Mcp-Name"), None);
    assert_eq!(list.body["params"]["cursor"], "c");
}

#[test]
fn ids_count_up_and_survive_the_fall_back_to_the_legacy_era() {
    let mut requests = Requests::new(Era::Modern);
    assert_eq!(
        requests
            .request("server/discover", Map::new(), Vec::new())
            .id,
        Some(1)
    );
    requests.fall_back();
    assert_eq!(requests.era(), Era::Legacy);
    let initialize = requests.initialize();
    assert_eq!(initialize.id, Some(2));
    assert_eq!(initialize.body["params"]["protocolVersion"], LEGACY);
    assert_eq!(initialize.body["params"]["capabilities"], json!({}));
    assert_eq!(initialize.body["params"].get("_meta"), None);
    assert_eq!(initialize.headers, []);
}

#[test]
fn a_legacy_request_is_bare_and_names_the_negotiated_revision_once_there_is_one() {
    let mut requests = Requests::new(Era::Legacy);
    let before = requests.request("ping", Map::new(), Vec::new());
    assert_eq!(
        before.body,
        json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
    );
    assert_eq!(before.headers, []);
    assert_eq!(requests.initialized().headers, []);
    requests.negotiated(LEGACY);
    let after = requests.request(
        "tools/list",
        Map::new(),
        vec![("Mcp-Param-X".to_owned(), "1".to_owned())],
    );
    assert_eq!(
        after.headers,
        [("MCP-Protocol-Version".to_owned(), LEGACY.to_owned())]
    );
    let initialized = requests.initialized();
    assert_eq!(initialized.id, None);
    assert_eq!(
        initialized.body,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    );
    assert_eq!(
        initialized.headers,
        [("MCP-Protocol-Version".to_owned(), LEGACY.to_owned())]
    );
}

#[test]
fn a_servers_ping_is_answered_and_any_other_request_refused() {
    let requests = Requests::new(Era::Legacy);
    let pong = requests.answer(&json!({"jsonrpc": "2.0", "id": "p1", "method": "ping"}));
    assert_eq!(
        pong.body,
        json!({"jsonrpc": "2.0", "id": "p1", "result": {}})
    );
    assert_eq!(pong.id, None);
    let refusal = requests.answer(&json!({"jsonrpc": "2.0", "id": 9, "method": "roots/list"}));
    assert_eq!(refusal.body["id"], 9);
    assert_eq!(refusal.body["error"]["code"], -32601);
    assert_eq!(refusal.body.get("result"), None);
}
