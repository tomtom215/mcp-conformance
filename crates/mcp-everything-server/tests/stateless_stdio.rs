// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `2026-07-28` stateless surface over stdio, against the real binary.
//!
//! Four modules' unit tests point here, and this is why: the envelope gate,
//! the capability gate and the MRTR round are each pure functions that their
//! own tests pin, but *whether rmcp delivers them what they need* is not a
//! property of any of them. `context.meta` arriving populated over a transport
//! with no headers, `inputResponses` surviving a retry, a notification
//! reaching the handler through the wrapper — each is plumbing, and plumbing
//! is only testable end to end.
//!
//! Raw JSON-RPC over the child's pipes rather than an rmcp client, so a change
//! in what rmcp's client *chooses* to send cannot quietly change what is being
//! asserted about the server.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// The `_meta` envelope every request carries at this revision.
const ENVELOPE: &str = r#""io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}"#;

/// A stateless stdio server, killed and reaped when it leaves scope —
/// including while a panic unwinds, so a failing assertion here cannot leak a
/// child process.
struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
            .args(["--transport", "stdio", "--protocol-version", "2026-07-28"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("binary spawns");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
        }
    }

    /// Writes one JSON-RPC message.
    fn send(&mut self, message: &str) {
        writeln!(self.stdin, "{message}").expect("write to the server");
    }

    /// Reads `count` answers, keyed by their JSON-RPC id.
    ///
    /// Correlated rather than ordered: a stateless server answers independent
    /// requests concurrently and has no reason to reply in the order it read
    /// them.
    fn answers(&mut self, count: usize) -> BTreeMap<u64, Value> {
        let mut answers = BTreeMap::new();
        while answers.len() < count {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read an answer");
            assert_ne!(read, 0, "the server closed stdout after {answers:?}");
            let answer: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("not one JSON line ({error}): {line}"));
            // Server-initiated messages carry a `method`; at this revision
            // there should be none, and a test that silently skipped them
            // would be unable to say so.
            assert!(
                answer.get("method").is_none(),
                "the server sent an independent message: {answer}"
            );
            answers.insert(answer["id"].as_u64().expect("an id"), answer);
        }
        answers
    }

    /// Reads everything a subscription produced: the notifications, in
    /// arrival order, and the final `subscriptions/listen` response.
    ///
    /// Ordering matters here in a way it does not for independent requests —
    /// SUBS-002 is about *which message came first* — so this keeps the
    /// sequence rather than keying by id.
    fn subscription(&mut self, id: u64) -> (Vec<Value>, Value) {
        let mut notifications = Vec::new();
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read");
            assert_ne!(read, 0, "the stream closed with no final result");
            let message: Value = serde_json::from_str(&line).expect("one JSON line");
            if message["id"].as_u64() == Some(id) && message.get("method").is_none() {
                return (notifications, message);
            }
            notifications.push(message);
        }
    }

    /// Sends one request and returns its answer.
    fn exchange(&mut self, message: &str, id: u64) -> Value {
        self.send(message);
        self.answers(1).remove(&id).expect("the answer")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A request for `method` carrying the envelope plus `extra` params.
fn request(id: u64, method: &str, extra: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"_meta":{{{ENVELOPE}}}{extra}}}}}"#
    )
}

#[test]
fn discovery_answers_without_a_handshake() {
    // The whole point of the revision, in one exchange: no `initialize`, and
    // the capability set arrives from `server/discover` instead.
    let mut server = Server::start();
    let answer = server.exchange(&request(1, "server/discover", ""), 1);
    let result = &answer["result"];
    assert_eq!(
        result["supportedVersions"],
        json!(["2026-07-28"]),
        "{answer}"
    );
    assert_eq!(result["resultType"], "complete", "{answer}");
    for capability in ["tools", "resources", "prompts", "logging", "completions"] {
        assert!(
            result["capabilities"].get(capability).is_some(),
            "{capability} must be declared: {result}"
        );
    }
    // SEP-2549: the answer is reusable and says so. Reached through the
    // envelope wrapper, so a wrapper that stopped delegating `get_info` would
    // be visible here rather than only in a capture.
    assert!(
        result["ttlMs"].as_u64().is_some_and(|ttl| ttl > 0),
        "{result}"
    );
    assert_eq!(result["cacheScope"], "public", "{result}");
    // BASE-037: the server identifies itself in the result's `_meta`, so a
    // client holding only this answer knows who produced it without any prior
    // connection state — which is the whole premise of a stateless revision.
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "mcp-everything-server",
        "{result}"
    );
}

#[test]
fn the_envelope_is_required_field_by_field() {
    // BASE-031, and the message names which field — a client that cannot tell
    // which of the two it omitted has to guess.
    let mut server = Server::start();
    server.send(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#);
    server.send(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
    );
    let answers = server.answers(2);
    assert_eq!(answers[&1]["error"]["code"], -32602, "{:?}", answers[&1]);
    assert!(
        answers[&1]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("protocolVersion"),
        "{:?}",
        answers[&1]
    );
    assert_eq!(answers[&2]["error"]["code"], -32602, "{:?}", answers[&2]);
    assert!(
        answers[&2]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("clientCapabilities"),
        "{:?}",
        answers[&2]
    );
}

#[test]
fn a_version_this_server_does_not_serve_is_refused_with_its_list() {
    // VERS-001. The list is what lets a client retry instead of giving up, and
    // rmcp's client drives its retry off exactly this field.
    let mut server = Server::start();
    let legacy = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2025-11-25","io.modelcontextprotocol/clientCapabilities":{}}}}"#;
    let answer = server.exchange(legacy, 1);
    assert_eq!(answer["error"]["code"], -32022, "{answer}");
    assert_eq!(
        answer["error"]["data"]["supported"],
        json!(["2026-07-28"]),
        "{answer}"
    );
}

#[test]
fn an_undeclared_capability_is_refused_in_the_schema_shape() {
    // BASE-035: `-32021`, and `data.requiredCapabilities` as a
    // `ClientCapabilities` object — the same shape the client would have sent.
    let mut server = Server::start();
    let answer = server.exchange(
        &request(
            1,
            "tools/call",
            r#","name":"test_sampling","arguments":{"prompt":"hi"}"#,
        ),
        1,
    );
    assert_eq!(answer["error"]["code"], -32021, "{answer}");
    assert_eq!(
        answer["error"]["data"]["requiredCapabilities"],
        json!({ "sampling": {} }),
        "{answer}"
    );
}

#[test]
fn a_declared_capability_is_read_from_the_request_that_declared_it() {
    // The gate must read *this request's* `_meta`, not session state: there is
    // none. A gate resolving through the handshake refuses a client that is
    // declaring the capability in the envelope it is looking straight at.
    let mut server = Server::start();
    let declaring = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"sampling":{}}},"name":"test_sampling","arguments":{"prompt":"hi"}}}"#;
    let answer = server.exchange(declaring, 1);
    assert!(
        answer.get("error").is_none(),
        "a declared capability must not be refused: {answer}"
    );
}

#[test]
fn an_interactive_tool_asks_through_mrtr_and_completes_on_the_retry() {
    // MRTR-001 and the transports' prohibition on independent server requests:
    // the ask is *returned*, and `Server::answers` fails the test if anything
    // arrives carrying a `method`. The retry then completes the same call.
    let mut server = Server::start();
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"elicitation":{}}},"name":"test_elicitation","arguments":{"message":"hi"}}}"#;
    let asked = server.exchange(call, 1);
    let result = &asked["result"];
    assert_eq!(result["resultType"], "input_required", "{asked}");
    let requests = result["inputRequests"].as_object().expect("inputRequests");
    assert_eq!(requests.len(), 1, "{result}");
    let (key, ask) = requests.iter().next().unwrap();
    // MRTR-006: the value is one of the three request types the pattern allows.
    assert_eq!(ask["method"], "elicitation/create", "{result}");
    let state = result["requestState"].as_str().expect("requestState");

    // MRTR-015/016/019: the answer under the server's own key, that exact
    // state echoed back, and a different id — an independent request.
    // The retry declares the capability again: each request is standalone, so
    // the gate applies to it exactly as it applied to the first round. A
    // client that declared once and then stopped would be refused, correctly.
    let retry = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{{"elicitation":{{}}}}}},"name":"test_elicitation","arguments":{{"message":"hi"}},"inputResponses":{{"{key}":{{"action":"accept","content":{{"username":"ada"}}}}}},"requestState":"{state}"}}}}"#
    );
    let completed = server.exchange(&retry, 2);
    assert_eq!(completed["result"]["resultType"], "complete", "{completed}");
    let text = completed["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    assert!(text.contains("action=accept"), "{text}");
    assert!(
        text.contains("ada"),
        "the client's answer reaches the result: {text}"
    );
}

#[test]
fn a_retry_missing_the_requested_input_is_asked_again() {
    // MRTR-024: the client retried without what the server asked for. Asking
    // again beats failing a call the client can still complete — and beats
    // reading whatever *is* in the map, which would consume an answer meant
    // for a different input request.
    let mut server = Server::start();
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"elicitation":{}}},"name":"test_elicitation","arguments":{"message":"hi"},"inputResponses":{"not-the-key":{"action":"accept"}},"requestState":"test_elicitation:1"}}"#;
    let answer = server.exchange(call, 1);
    assert_eq!(answer["result"]["resultType"], "input_required", "{answer}");
    assert_eq!(
        answer["result"]["requestState"], "test_elicitation:2",
        "the round advances, so a trace shows two asks rather than one: {answer}"
    );
}

#[test]
fn logging_rides_only_a_request_that_asked_for_it() {
    // LOG-008 is a MUST NOT and the default is silence: `logging/setLevel` is
    // gone, so a request that named no level gets nothing.
    let mut server = Server::start();
    let silent = server.exchange(
        &request(1, "tools/call", r#","name":"test_tool_with_logging""#),
        1,
    );
    assert!(silent.get("error").is_none(), "{silent}");

    // And a request that asked gets them, on its own answer stream. The
    // notifications arrive before the result, so they are read here rather
    // than through `answers`, which rejects anything carrying a `method`.
    let asking = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"_meta":{{{ENVELOPE},"io.modelcontextprotocol/logLevel":"info"}},"name":"test_tool_with_logging"}}}}"#
    );
    server.send(&asking);
    let mut logs = 0;
    loop {
        let mut line = String::new();
        server.stdout.read_line(&mut line).expect("read");
        let message: Value = serde_json::from_str(&line).expect("one JSON line");
        if message["method"] == "notifications/message" {
            logs += 1;
        } else if message["id"] == 2 {
            break;
        }
    }
    assert!(
        logs > 0,
        "a request that asked for info-level logs got none"
    );
}

/// The subscription id a notification carries, if any.
fn subscription_id(notification: &Value) -> Option<u64> {
    notification["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"].as_u64()
}

#[test]
fn a_subscription_acknowledges_first_and_closes_gracefully() {
    // The whole lifecycle in one exchange, because the clauses are about the
    // *sequence*: acknowledgment first (SUBS-002), then only what was asked
    // for (SUBS-001), then an empty final result (SUBS-005, SUBS-006).
    let mut server = Server::start();
    let listen = r#"{"jsonrpc":"2.0","id":7,"method":"subscriptions/listen","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}},"notifications":{"toolsListChanged":true,"resourceSubscriptions":["test://static-text","test://not-a-resource"]}}}"#;
    server.send(listen);
    let (notifications, closed) = server.subscription(7);

    let methods: Vec<&str> = notifications
        .iter()
        .map(|notification| notification["method"].as_str().expect("a method"))
        .collect();
    assert_eq!(
        methods,
        [
            "notifications/subscriptions/acknowledged",
            "notifications/tools/list_changed",
            "notifications/resources/updated",
        ],
        "the acknowledgment is first, and nothing unrequested follows: {notifications:?}"
    );

    // BASE-039: every one of them names the subscription, which on stdio is
    // the only way a client can tell which stream a message belongs to.
    for notification in &notifications {
        assert_eq!(
            subscription_id(notification),
            Some(7),
            "untagged: {notification}"
        );
    }

    // SUBS-003's premise: the acknowledgment reports what the server will
    // actually send, which is not everything that was asked for.
    let acknowledged = &notifications[0]["params"]["notifications"];
    assert_eq!(acknowledged["toolsListChanged"], true, "{acknowledged}");
    assert_eq!(
        acknowledged["resourceSubscriptions"],
        json!(["test://static-text"]),
        "a resource this server does not have must not be acknowledged: {acknowledged}"
    );

    // SUBS-005/006: an empty result — nothing beyond `resultType` and the
    // `_meta` naming the subscription.
    let result = closed["result"].as_object().expect("a result object");
    let mut keys: Vec<&str> = result.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["_meta", "resultType"], "{closed}");
    assert_eq!(result["resultType"], "complete", "{closed}");
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/subscriptionId"], 7,
        "{closed}"
    );
}

#[test]
fn a_subscription_that_asked_for_nothing_receives_nothing() {
    // SUBS-001 from the other side: an empty filter is not a shorthand for
    // "everything", which is the failure mode a default-on server would have.
    let mut server = Server::start();
    let listen = r#"{"jsonrpc":"2.0","id":1,"method":"subscriptions/listen","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}},"notifications":{}}}"#;
    server.send(listen);
    let (notifications, closed) = server.subscription(1);
    let methods: Vec<&str> = notifications
        .iter()
        .map(|notification| notification["method"].as_str().expect("a method"))
        .collect();
    assert_eq!(
        methods,
        ["notifications/subscriptions/acknowledged"],
        "only the acknowledgment: {notifications:?}"
    );
    assert_eq!(closed["result"]["resultType"], "complete", "{closed}");
}

#[test]
fn the_tools_that_belong_to_the_older_model_are_not_offered() {
    // `test_list_changed` broadcasts the three `list_changed` notifications
    // outside any subscription — the model `subscriptions/listen` replaced.
    // At this revision they belong to a subscription and carry its id, which
    // is what the announcement above delivers.
    let mut server = Server::start();
    let answer = server.exchange(&request(1, "tools/list", ""), 1);
    let tools = answer["result"]["tools"].as_array().expect("tools");
    for retired in ["test_url_elicitation", "test_list_changed"] {
        assert!(
            !tools.iter().any(|tool| tool["name"] == retired),
            "{retired} must not be listed: {answer}"
        );
    }
}
