// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The designated-header probe: a `tools/call` that leaves one `Mcp-Param-*`
//! header out.
//!
//! The header table's last row — "Client omits header but value is in body |
//! Non-conforming client | Server MUST reject the request" (`TRAN-136`) — is
//! the one row a conforming client can never show: rmcp mirrors every
//! designated argument it sends. So the probe sends the call rmcp will not.
//!
//! Unlike the static probes, this one has to *discover* what to call. A
//! designation binds only through a tool definition the recording carries, so
//! the probe lists the server's tools first — following `nextCursor` as given,
//! an opaque token handed back verbatim — and picks the first tool with a
//! required designated argument. A server offering none draws no call, and
//! the outcome says so instead of inventing a tool name.

use serde_json::{Map, Value};

use super::{ENVELOPE, ProbeOutcome, REVISION, TIMEOUT};
use crate::run::synthesize_arguments;

/// The first id this phase uses, past every static probe's.
const FIRST_ID: u64 = 10;

/// Pages listed before giving up, so a server whose cursors never end cannot
/// hold the probe forever.
const PAGE_LIMIT: u64 = 8;

/// What the listing phase is, in the run record.
const LISTING: &str = "TRAN-136 (setup)";

/// The probe's own fault, in the run record.
const FAULT: &str = "a designated argument sent without its Mcp-Param header";

/// Lists the server's tools, then calls one with a designated header omitted.
pub(super) async fn run(client: &reqwest::Client, url: &str) -> Vec<ProbeOutcome> {
    let mut outcomes = Vec::new();
    let (tools, id) = list_tools(client, url, &mut outcomes).await;
    let Some(call) = tools.iter().find_map(omitted_header_call) else {
        outcomes.push(ProbeOutcome {
            clauses: "TRAN-136",
            fault: FAULT,
            answer: Err(
                "skipped: the server lists no tool with a required designated argument".to_owned(),
            ),
        });
        return outcomes;
    };
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"_meta":{{{ENVELOPE}}},"name":{},"arguments":{}}}}}"#,
        Value::String(call.tool.clone()),
        Value::Object(call.arguments)
    );
    let (answer, _) = post(
        client,
        url,
        "tools/call",
        Some(&call.tool),
        &call.headers,
        body,
    )
    .await;
    outcomes.push(ProbeOutcome {
        clauses: "TRAN-136",
        fault: FAULT,
        answer,
    });
    outcomes
}

/// Every tool the server lists, page by page, recording each listing; returns
/// them with the next unused request id.
async fn list_tools(
    client: &reqwest::Client,
    url: &str,
    outcomes: &mut Vec<ProbeOutcome>,
) -> (Vec<Value>, u64) {
    let mut tools = Vec::new();
    let mut cursor: Option<String> = None;
    let mut id = FIRST_ID;
    while id < FIRST_ID + PAGE_LIMIT {
        let body = listing(id, cursor.as_deref());
        id += 1;
        let (answer, payload) = post(client, url, "tools/list", None, &[], body).await;
        outcomes.push(ProbeOutcome {
            clauses: LISTING,
            fault: "none: lists the tools, so the recording carries their designations",
            answer,
        });
        let result = payload.as_ref().and_then(|payload| payload.get("result"));
        tools.extend(
            result
                .and_then(|result| result.get("tools"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .cloned(),
        );
        cursor = result
            .and_then(|result| result.get("nextCursor"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    (tools, id)
}

/// A `tools/list` body, presenting `cursor` when there is one.
fn listing(id: u64, cursor: Option<&str>) -> String {
    let cursor = cursor.map_or_else(String::new, |cursor| {
        format!(r#","cursor":{}"#, Value::String(cursor.to_owned()))
    });
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{"_meta":{{{ENVELOPE}}}{cursor}}}}}"#
    )
}

/// The call this phase sends: a tool's name, its arguments, and every
/// `Mcp-Param-*` header they need but one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OmittedHeaderCall {
    /// The tool called.
    pub tool: String,
    /// Its arguments, synthesized from the schema as the tool loop does.
    pub arguments: Map<String, Value>,
    /// The headers sent: every designated argument's but one.
    pub headers: Vec<(String, String)>,
}

/// The call that omits one designated header for `tool`, when it has a
/// required designated argument to omit it for.
///
/// The arguments are the required ones only, as the tool loop sends them, so
/// every designated argument in the body has a value; the one whose header is
/// left out is the first by property name, which makes the choice a function
/// of the tool definition rather than of anything about this run.
pub(super) fn omitted_header_call(tool: &Value) -> Option<OmittedHeaderCall> {
    let name = tool.get("name")?.as_str()?;
    let schema = tool.get("inputSchema")?.as_object()?;
    let properties = schema.get("properties")?.as_object()?;
    let arguments = synthesize_arguments(schema);
    let designated: Vec<(&String, &str)> = properties
        .iter()
        .filter(|(property, _)| arguments.contains_key(*property))
        .filter_map(|(property, definition)| {
            Some((property, definition.get("x-mcp-header")?.as_str()?))
        })
        .collect();
    let (_, mirrored) = designated.split_first()?;
    let headers = mirrored
        .iter()
        .filter_map(|(property, header)| {
            let value = match arguments.get(*property)? {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            Some((format!("mcp-param-{}", header.to_ascii_lowercase()), value))
        })
        .collect();
    Some(OmittedHeaderCall {
        tool: name.to_owned(),
        arguments,
        headers,
    })
}

/// POSTs `body` for `method`, returning the status and the JSON-RPC message
/// the response carried, read through either framing a server may choose.
async fn post(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    name: Option<&str>,
    extra: &[(String, String)],
    body: String,
) -> (Result<u16, String>, Option<Value>) {
    let mut request = client
        .post(url)
        .timeout(TIMEOUT)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-method", method)
        .header("mcp-protocol-version", REVISION);
    if let Some(name) = name {
        request = request.header("mcp-name", name);
    }
    for (header, value) in extra {
        request = request.header(header.as_str(), value.as_str());
    }
    let response = match request.body(body).send().await {
        Ok(response) => response,
        Err(error) => return (Err(error.to_string()), None),
    };
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (Ok(status), message_in(&text))
}

/// The JSON-RPC message in a response body: the `data:` line of a one-event
/// SSE stream, or the body itself.
fn message_in(text: &str) -> Option<Value> {
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .map_or(text, str::trim_start);
    serde_json::from_str(payload).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_first_required_designation_is_the_one_left_out() {
        let tool = json!({
            "name": "t",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "x-mcp-header": "Limit"},
                    "region": {"type": "string", "x-mcp-header": "Region"},
                    "zone": {"type": "string", "x-mcp-header": "Zone"}
                },
                "required": ["region", "limit"]
            }
        });
        let call = omitted_header_call(&tool).unwrap();
        assert_eq!(call.tool, "t");
        // Both required arguments are in the body, the optional one is not...
        assert_eq!(
            Value::Object(call.arguments),
            json!({"region": "probe", "limit": 7})
        );
        // ...and `limit`, first by name, goes without its header.
        assert_eq!(
            call.headers,
            [("mcp-param-region".to_owned(), "probe".to_owned())]
        );
    }

    #[test]
    fn a_tool_without_a_required_designation_offers_no_call() {
        for tool in [
            json!({"name": "plain", "inputSchema": {"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]}}),
            json!({"name": "optional", "inputSchema": {"type": "object", "properties": {"z": {"type": "string", "x-mcp-header": "Z"}}}}),
            json!({"name": "no-schema"}),
        ] {
            assert_eq!(omitted_header_call(&tool), None, "{tool}");
        }
    }

    #[test]
    fn listings_present_the_cursor_verbatim() {
        let first: Value = serde_json::from_str(&listing(10, None)).unwrap();
        assert!(first["params"].get("cursor").is_none());
        let next: Value = serde_json::from_str(&listing(11, Some("opaque \"token\""))).unwrap();
        assert_eq!(next["params"]["cursor"], "opaque \"token\"");
        assert_eq!(
            next["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
            REVISION
        );
    }

    #[test]
    fn a_response_is_read_through_either_framing() {
        let message = json!({"jsonrpc": "2.0", "id": 1, "result": {}});
        assert_eq!(message_in(&message.to_string()), Some(message.clone()));
        let sse = format!("event: message\ndata: {message}\n\n");
        assert_eq!(message_in(&sse), Some(message));
        assert_eq!(message_in("not json"), None);
    }
}
