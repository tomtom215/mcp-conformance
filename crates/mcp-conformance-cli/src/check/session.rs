// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The exerciser: what a conforming client does with a server it knows
//! nothing about, within the run's bounds (ADR-0024).
//!
//! It asks only for what the server declared, addresses only what the server
//! listed, reads freely, and calls a tool only when the server marks it
//! read-only or the user named it. Everything it leaves alone it names, with
//! the reason, in the [`Summary`].

use std::time::Instant;

use serde_json::{Map, Value, json};

use super::request::{Era, LEGACY, MODERN, Requests};
use super::summary::{Kind, Skipped, Summary};
use super::wire::{Failure, Limits, Wire};
use super::{arguments, headers};

mod listing;
mod probes;

/// What the run may do.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// Tools to call whatever their annotations say (`--call`).
    pub(crate) call: Vec<String>,
    /// Call every listed tool (`--call-all`).
    pub(crate) call_all: bool,
    /// Send the probes (off with `--no-probes`).
    pub(crate) probes: bool,
    pub(crate) max_requests: usize,
    pub(crate) max_tool_calls: usize,
    pub(crate) max_reads: usize,
    pub(crate) max_pages: usize,
    /// When the whole run must be over.
    pub(crate) deadline: Instant,
    pub(crate) limits: Limits,
}

/// Why the session went no further.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ended {
    /// A bound of the [`Plan`] was reached; the summary names it.
    Bound,
    /// The server is gone, or the transport failed.
    Lost(Failure),
    /// The server negotiated a revision this client does not speak.
    Unsupported(String),
}

/// A server's answer to one request.
enum Reply {
    Result(Value),
    Error,
    /// No JSON-RPC answer, and the session can go on.
    Nothing,
}

/// A session being driven.
pub(crate) struct Session<'a> {
    pub(crate) wire: &'a mut Wire,
    pub(crate) requests: Requests,
    pub(crate) plan: &'a Plan,
    pub(crate) summary: &'a mut Summary,
}

impl Session<'_> {
    /// One request and its answer, counted against the bounds.
    async fn ask(
        &mut self,
        method: &str,
        params: Map<String, Value>,
        mirrored: Vec<(String, String)>,
        what: &str,
    ) -> Result<Reply, Ended> {
        if self.summary.requests >= self.plan.max_requests {
            return Err(self.bound("the request budget (--max-requests)"));
        }
        if Instant::now() >= self.plan.deadline {
            return Err(self.bound("the time budget (--max-time)"));
        }
        let out = self.requests.request(method, params, mirrored);
        self.summary.requests += 1;
        match self
            .wire
            .exchange(&out, &self.requests, self.plan.limits)
            .await
        {
            Ok(answer) => {
                self.summary.server_requests += answer.server_requests;
                if let (Wire::Http(http), Some(session)) = (&mut *self.wire, answer.session) {
                    http.session(Some(session));
                }
                Ok(answer
                    .message
                    .get("result")
                    .cloned()
                    .map_or(Reply::Error, Reply::Result))
            }
            Err(failure @ (Failure::Closed | Failure::Transport(_))) => {
                self.summary.unanswered.push(format!("{what}: {failure}"));
                Err(Ended::Lost(failure))
            }
            Err(failure) => {
                self.summary.unanswered.push(format!("{what}: {failure}"));
                Ok(Reply::Nothing)
            }
        }
    }

    fn bound(&mut self, name: &str) -> Ended {
        self.summary
            .stopped_by
            .get_or_insert_with(|| name.to_owned());
        Ended::Bound
    }

    /// Probes with `server/discover`: the capabilities when the server speaks
    /// the modern revision, `None` when the session should fall back to the
    /// legacy handshake.
    pub(crate) async fn discover(&mut self) -> Result<Option<Value>, Ended> {
        match self
            .ask("server/discover", Map::new(), Vec::new(), "server/discover")
            .await?
        {
            Reply::Result(result) => {
                let modern = result
                    .get("supportedVersions")
                    .and_then(Value::as_array)
                    .is_some_and(|versions| versions.iter().any(|version| version == MODERN));
                if !modern {
                    return Ok(None);
                }
                self.summary.revision = Some(MODERN.to_owned());
                self.summary.server =
                    identity(result.pointer("/_meta/io.modelcontextprotocol~1serverInfo"));
                Ok(Some(
                    result.get("capabilities").cloned().unwrap_or_default(),
                ))
            }
            Reply::Error | Reply::Nothing => Ok(None),
        }
    }

    /// The legacy handshake: the capabilities the server declared.
    pub(crate) async fn initialize(&mut self) -> Result<Value, Ended> {
        self.requests.fall_back();
        let out = self.requests.initialize();
        self.summary.requests += 1;
        let answer = self
            .wire
            .exchange(&out, &self.requests, self.plan.limits)
            .await
            .map_err(|failure| {
                self.summary
                    .unanswered
                    .push(format!("initialize: {failure}"));
                Ended::Lost(failure)
            })?;
        // The handshake's exchange is made here, not through `ask`, so what it
        // showed is added here too: a server may send a request before it
        // answers `initialize`.
        self.summary.server_requests += answer.server_requests;
        let Some(result) = answer.message.get("result") else {
            let refusal = answer
                .message
                .pointer("/error/message")
                .and_then(Value::as_str);
            let failure = Failure::Transport(format!(
                "the server refused `initialize`: {}",
                refusal.unwrap_or("it gave no reason")
            ));
            self.summary
                .unanswered
                .push(format!("initialize: {failure}"));
            return Err(Ended::Lost(failure));
        };
        let revision = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        self.summary.server = identity(result.get("serverInfo"));
        if revision != LEGACY {
            // A client that does not speak the revision the server chose
            // disconnects; it does not carry on under rules it cannot follow.
            return Err(Ended::Unsupported(revision));
        }
        self.summary.revision = Some(revision.clone());
        self.requests.negotiated(&revision);
        if let (Wire::Http(http), Some(session)) = (&mut *self.wire, answer.session) {
            http.session(Some(session));
        }
        let initialized = self.requests.initialized();
        self.wire
            .notify(&initialized, self.plan.limits)
            .await
            .map_err(Ended::Lost)?;
        Ok(result.get("capabilities").cloned().unwrap_or_default())
    }

    /// Everything after the session is established.
    pub(crate) async fn exercise(&mut self, capabilities: &Value) -> Result<(), Ended> {
        if self.requests.era() == Era::Legacy {
            self.ask("ping", Map::new(), Vec::new(), "ping").await?;
        }
        if capabilities.get("tools").is_some() {
            self.summary.tools.declared = true;
            let tools = self.list("tools/list", "tools").await?;
            self.summary.tools.listed = tools.len();
            self.named_but_unlisted(&tools);
            for tool in &tools {
                self.call(tool).await?;
            }
        }
        if capabilities.get("resources").is_some() {
            self.summary.resources.declared = true;
            let resources = self.list("resources/list", "resources").await?;
            let templates = self
                .list("resources/templates/list", "resourceTemplates")
                .await?;
            self.summary.resources.listed = resources.len();
            self.read(&resources, &templates).await?;
            self.probe_resources(&resources, &templates).await?;
        }
        if capabilities.get("prompts").is_some() {
            self.summary.prompts.declared = true;
            let prompts = self.list("prompts/list", "prompts").await?;
            self.summary.prompts.listed = prompts.len();
            self.fetch(&prompts).await?;
            self.probe_prompts(&prompts).await?;
        }
        self.probe_utilities(capabilities).await
    }

    fn skip(&mut self, kind: &'static str, name: &str, reason: impl Into<String>) {
        self.summary.skipped.push(Skipped {
            kind,
            name: name.to_owned(),
            reason: reason.into(),
        });
    }

    /// `--call` names the server never listed are said, not sent: a call to
    /// an unlisted name is not something a conforming client makes.
    fn named_but_unlisted(&mut self, tools: &[Value]) {
        let unlisted: Vec<String> = self
            .plan
            .call
            .iter()
            .filter(|name| {
                !tools
                    .iter()
                    .any(|tool| tool.get("name") == Some(&json!(name)))
            })
            .cloned()
            .collect();
        for name in unlisted {
            self.skip(
                "tool",
                &name,
                "named with --call, and the server does not list it",
            );
        }
    }

    /// Calls one tool, if the plan allows and arguments can be built.
    async fn call(&mut self, tool: &Value) -> Result<(), Ended> {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or_default();
        let schema = tool.get("inputSchema").cloned().unwrap_or_default();
        let over_http = matches!(self.wire, Wire::Http(_)) && self.requests.era() == Era::Modern;
        let designated = if over_http {
            match headers::designated(&schema) {
                Ok(designated) => designated,
                Err(reason) => {
                    self.skip(
                        "tool",
                        name,
                        format!(
                            "its definition is invalid, so a client must leave it out: {reason}"
                        ),
                    );
                    return Ok(());
                }
            }
        } else {
            Vec::new()
        };
        let asked = self.plan.call_all || self.plan.call.iter().any(|named| named == name);
        if !asked && !read_only(tool) {
            self.skip(
                "tool",
                name,
                "not annotated read-only (`readOnlyHint: true`); name it with --call, or use --call-all",
            );
            return Ok(());
        }
        let built = arguments::build(&schema)
            .and_then(|arguments| Ok((headers::mirrored(&designated, &arguments)?, arguments)));
        let (mirrored, arguments) = match built {
            Ok(built) => built,
            Err(reason) => {
                self.skip("tool", name, format!("no arguments were built: {reason}"));
                return Ok(());
            }
        };
        if self.summary.tools.exercised >= self.plan.max_tool_calls {
            self.skip("tool", name, "over the budget (--max-tool-calls)");
            return Ok(());
        }
        let mut params = Map::new();
        params.insert("name".to_owned(), json!(name));
        params.insert("arguments".to_owned(), arguments);
        let what = format!("tools/call {name}");
        let reply = self.ask("tools/call", params, mirrored, &what).await?;
        tally(&mut self.summary.tools, &reply);
        self.note_input(&reply, &what);
        Ok(())
    }

    /// Reads the listed resources, up to the budget. Templates are named and
    /// left: nothing says what values would fill them.
    async fn read(&mut self, resources: &[Value], templates: &[Value]) -> Result<(), Ended> {
        for template in templates {
            let name = template
                .get("uriTemplate")
                .and_then(Value::as_str)
                .unwrap_or_default();
            self.skip(
                "resource template",
                name,
                "a template needs values, and nothing states them",
            );
        }
        for resource in resources {
            let uri = resource
                .get("uri")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if self.summary.resources.exercised >= self.plan.max_reads {
                self.skip("resource", uri, "over the budget (--max-reads)");
                continue;
            }
            let mut params = Map::new();
            params.insert("uri".to_owned(), json!(uri));
            let what = format!("resources/read {uri}");
            let reply = self
                .ask("resources/read", params, Vec::new(), &what)
                .await?;
            tally(&mut self.summary.resources, &reply);
            self.note_input(&reply, &what);
        }
        Ok(())
    }

    /// Gets the prompts that need no arguments. A prompt's arguments have
    /// names and no schema, so there is nothing to build one from.
    async fn fetch(&mut self, prompts: &[Value]) -> Result<(), Ended> {
        for prompt in prompts {
            let name = prompt
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let needs_arguments = prompt
                .get("arguments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|argument| argument.get("required") == Some(&Value::Bool(true)));
            if needs_arguments {
                self.skip(
                    "prompt",
                    name,
                    "it requires arguments, which have names and no schema to build a value from",
                );
                continue;
            }
            if self.summary.prompts.exercised >= self.plan.max_reads {
                self.skip("prompt", name, "over the budget (--max-reads)");
                continue;
            }
            let mut params = Map::new();
            params.insert("name".to_owned(), json!(name));
            let what = format!("prompts/get {name}");
            let reply = self.ask("prompts/get", params, Vec::new(), &what).await?;
            tally(&mut self.summary.prompts, &reply);
            self.note_input(&reply, &what);
        }
        Ok(())
    }

    /// Notes a step the server answered by asking for client input.
    fn note_input(&mut self, reply: &Reply, what: &str) {
        if let Reply::Result(result) = reply
            && result.get("resultType") == Some(&json!("input_required"))
        {
            self.summary.needed_input.push(what.to_owned());
        }
    }
}

/// Whether the server marks `tool` as one that changes nothing.
fn read_only(tool: &Value) -> bool {
    let annotations = tool.get("annotations");
    let hint = |name: &str| annotations.and_then(|all| all.get(name)) == Some(&Value::Bool(true));
    hint("readOnlyHint") && !hint("destructiveHint")
}

const fn tally(kind: &mut Kind, reply: &Reply) {
    match reply {
        Reply::Result(_) => kind.exercised += 1,
        Reply::Error => {
            kind.exercised += 1;
            kind.errors += 1;
        }
        Reply::Nothing => {}
    }
}

/// `name version`, from a server's `serverInfo`.
fn identity(info: Option<&Value>) -> Option<String> {
    let info = info?;
    let name = info.get("name").and_then(Value::as_str)?;
    let version = info
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(format!("{name} {version}").trim_end().to_owned())
}

#[cfg(test)]
mod tests;
