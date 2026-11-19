// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The messages the exerciser sends, for each era of the protocol.
//!
//! Two eras, as the `2026-07-28` versioning page names them: *legacy*, where a
//! session opens with the `initialize` handshake (`2025-11-25`), and *modern*,
//! where every request carries its own `_meta` envelope and, over HTTP, the
//! request-metadata headers. Every message here is one a conforming client
//! sends; nothing is malformed on purpose (ADR-0024, decision 2).

use serde_json::{Map, Value, json};

use super::headers;

/// The modern revision this client speaks.
pub(crate) const MODERN: &str = "2026-07-28";
/// The legacy revision this client proposes in `initialize`.
pub(crate) const LEGACY: &str = "2025-11-25";

/// How a session's requests are framed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Era {
    /// The `initialize` handshake, then bare requests.
    Legacy,
    /// Per-request `_meta` and request-metadata headers.
    Modern,
}

/// One message ready to send.
#[derive(Debug, Clone)]
pub(crate) struct Outgoing {
    /// The JSON-RPC message.
    pub(crate) body: Value,
    /// Its id; `None` for a notification.
    pub(crate) id: Option<u64>,
    /// The headers its era requires over HTTP, beyond `Accept`,
    /// `Content-Type` and the session's own.
    pub(crate) headers: Vec<(String, String)>,
}

/// Builds a session's messages, numbering its requests.
#[derive(Debug)]
pub(crate) struct Requests {
    era: Era,
    next: u64,
    /// The revision `initialize` negotiated, once it has.
    negotiated: Option<String>,
}

impl Requests {
    pub(crate) const fn new(era: Era) -> Self {
        Self {
            era,
            next: 1,
            negotiated: None,
        }
    }

    pub(crate) const fn era(&self) -> Era {
        self.era
    }

    /// Continues the session in the legacy era, after a modern probe the
    /// server did not answer as a modern server does. The numbering goes on:
    /// an id is used once in a session, whatever the era.
    pub(crate) const fn fall_back(&mut self) {
        self.era = Era::Legacy;
    }

    /// Records the revision the server's `initialize` result named, which
    /// every later request names in `MCP-Protocol-Version`.
    pub(crate) fn negotiated(&mut self, revision: &str) {
        self.negotiated = Some(revision.to_owned());
    }

    /// A request for `method` with `params`. `mirrored` are the `Mcp-Param-*`
    /// headers of a `tools/call`.
    pub(crate) fn request(
        &mut self,
        method: &str,
        mut params: Map<String, Value>,
        mirrored: Vec<(String, String)>,
    ) -> Outgoing {
        let id = self.next;
        self.next += 1;
        let mut headers = Vec::new();
        match self.era {
            Era::Modern => {
                params.insert("_meta".to_owned(), envelope());
                headers.push(("MCP-Protocol-Version".to_owned(), MODERN.to_owned()));
                headers.push(("Mcp-Method".to_owned(), method.to_owned()));
                let named = match method {
                    "tools/call" | "prompts/get" => params.get("name"),
                    "resources/read" => params.get("uri"),
                    _ => None,
                };
                if let Some(name) = named.and_then(Value::as_str) {
                    headers.push(("Mcp-Name".to_owned(), headers::encode(name)));
                }
                headers.extend(mirrored);
            }
            Era::Legacy => {
                if let Some(revision) = &self.negotiated {
                    headers.push(("MCP-Protocol-Version".to_owned(), revision.clone()));
                }
            }
        }
        let mut body = Map::new();
        body.insert("jsonrpc".to_owned(), json!("2.0"));
        body.insert("id".to_owned(), json!(id));
        body.insert("method".to_owned(), json!(method));
        if !params.is_empty() {
            body.insert("params".to_owned(), Value::Object(params));
        }
        Outgoing {
            body: Value::Object(body),
            id: Some(id),
            headers,
        }
    }

    /// The legacy handshake's opening request. This client declares no
    /// capabilities, so a conforming server asks it for nothing (ADR-0024,
    /// decision 5).
    pub(crate) fn initialize(&mut self) -> Outgoing {
        let mut params = Map::new();
        params.insert("protocolVersion".to_owned(), json!(LEGACY));
        params.insert("capabilities".to_owned(), json!({}));
        params.insert("clientInfo".to_owned(), client_info());
        self.request("initialize", params, Vec::new())
    }

    /// `notifications/initialized`, which closes the legacy handshake.
    pub(crate) fn initialized(&self) -> Outgoing {
        Outgoing {
            body: json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            id: None,
            headers: self.session_headers(),
        }
    }

    /// The answer to a request the server sent this client: an empty result
    /// for `ping`, which every party answers, and "method not found" for
    /// anything else, since this client declared no capability a server could
    /// be using.
    pub(crate) fn answer(&self, request: &Value) -> Outgoing {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let body = if request.get("method").and_then(Value::as_str) == Some("ping") {
            json!({"jsonrpc": "2.0", "id": id, "result": {}})
        } else {
            json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32601,
                "message": "Method not found: this client declared no capabilities",
            }})
        };
        Outgoing {
            body,
            id: None,
            headers: self.session_headers(),
        }
    }

    fn session_headers(&self) -> Vec<(String, String)> {
        self.negotiated
            .iter()
            .map(|revision| ("MCP-Protocol-Version".to_owned(), revision.clone()))
            .collect()
    }
}

/// Who this client says it is.
fn client_info() -> Value {
    json!({"name": "mcp-conformance-check", "version": env!("CARGO_PKG_VERSION")})
}

/// The `_meta` every modern request carries.
fn envelope() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": client_info(),
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

#[cfg(test)]
mod tests;
