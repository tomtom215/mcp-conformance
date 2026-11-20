// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Probes: requests a client would not send in the ordinary run of things,
//! sent on purpose because the specification says how a server answers them.
//!
//! The validator judges an error-handling clause only on a session that
//! carries its stimulus — a prompt nobody listed, a resource under a scheme
//! the server never named, a log level that is not one. A session that only
//! asks for what the server offers never carries them, so those clauses would
//! stay *not observed* for every server `check` meets. Each probe here is one
//! such stimulus.
//!
//! All of them ask for something that is not there, or ask wrongly: none can
//! change anything on a server. They are sent after the ordinary exercise of
//! each capability, each counted against the request budget, and each named
//! in the summary with how the server answered. `--no-probes` leaves them out.
//!
//! Two of them (the log level, the cursor) are requests the revision's schema
//! rejects, so the report holds them against this client, as it should; the
//! verdict is the server's, and is not moved by them.

use serde_json::{Map, Value, json};

use super::{Ended, Reply, Session};
use crate::check::request::Era;
use crate::check::summary::Probe;

/// A prompt name no server is expected to list.
const NO_SUCH_PROMPT: &str = "mcp-conformance-probe-no-such-prompt";

/// A resource under a scheme no server is expected to serve. The last
/// segment is not ASCII, so over HTTP at `2026-07-28` its `Mcp-Name` header
/// travels encoded, which a server must decode before comparing.
const NO_SUCH_RESOURCE: &str = "mcp-conformance-probe://no-such-resource/caf\u{e9}";

/// The scheme of [`NO_SUCH_RESOURCE`].
const PROBE_SCHEME: &str = "mcp-conformance-probe:";

/// A log level that is none of the eight.
const NO_SUCH_LEVEL: &str = "mcp-conformance-probe";

/// A listed prompt's name.
fn name_of(prompt: &Value) -> Option<&str> {
    prompt.get("name").and_then(Value::as_str)
}

impl Session<'_> {
    /// Whether probes are sent: asked for, and every listing so far was
    /// followed to its end (a name is only known to be unlisted then).
    const fn probing(&self) -> bool {
        self.plan.probes && self.summary.stopped_by.is_none()
    }

    /// One probe: `method` with `params`, recorded with its answer.
    async fn probe(
        &mut self,
        what: &str,
        method: &str,
        params: Map<String, Value>,
    ) -> Result<(), Ended> {
        let reply = self.ask(method, params, Vec::new(), what).await?;
        self.summary.probes.push(Probe {
            what: what.to_owned(),
            answer: match reply {
                Reply::Result(_) => "served",
                Reply::Error => "refused with an error",
                Reply::Nothing => "not answered",
            },
        });
        Ok(())
    }

    /// A prompt that is not listed, and a listed one without the arguments
    /// it requires.
    pub(super) async fn probe_prompts(&mut self, prompts: &[Value]) -> Result<(), Ended> {
        if !self.probing() {
            return Ok(());
        }
        if !prompts
            .iter()
            .any(|prompt| name_of(prompt) == Some(NO_SUCH_PROMPT))
        {
            let mut params = Map::new();
            params.insert("name".to_owned(), json!(NO_SUCH_PROMPT));
            self.probe(
                "prompts/get for a prompt the server does not list",
                "prompts/get",
                params,
            )
            .await?;
        }
        let demanding = prompts.iter().find_map(|prompt| {
            let required = prompt
                .get("arguments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|argument| argument.get("required") == Some(&Value::Bool(true)));
            name_of(prompt).filter(|_| required)
        });
        if let Some(name) = demanding {
            let mut params = Map::new();
            params.insert("name".to_owned(), json!(name));
            self.probe(
                &format!("prompts/get {name} without the arguments it requires"),
                "prompts/get",
                params,
            )
            .await?;
        }
        Ok(())
    }

    /// A resource under a scheme the server names nowhere.
    pub(super) async fn probe_resources(
        &mut self,
        resources: &[Value],
        templates: &[Value],
    ) -> Result<(), Ended> {
        let named = |items: &[Value], member: &str| {
            items.iter().any(|item| {
                item.get(member)
                    .and_then(Value::as_str)
                    .is_some_and(|uri| uri.starts_with(PROBE_SCHEME))
            })
        };
        if !self.probing() || named(resources, "uri") || named(templates, "uriTemplate") {
            return Ok(());
        }
        let mut params = Map::new();
        params.insert("uri".to_owned(), json!(NO_SUCH_RESOURCE));
        self.probe(
            "resources/read for a resource the server does not have",
            "resources/read",
            params,
        )
        .await
    }

    /// The utilities: a log level that is not one, a completion the server
    /// did not offer or cannot resolve, and a cursor that is not a string.
    pub(super) async fn probe_utilities(&mut self, capabilities: &Value) -> Result<(), Ended> {
        if !self.probing() {
            return Ok(());
        }
        let legacy = self.requests.era() == Era::Legacy;
        if legacy && capabilities.get("logging").is_some() {
            let mut params = Map::new();
            params.insert("level".to_owned(), json!(NO_SUCH_LEVEL));
            self.probe(
                "logging/setLevel with a level that is not one",
                "logging/setLevel",
                params,
            )
            .await?;
        }
        let mut params = Map::new();
        params.insert(
            "ref".to_owned(),
            json!({ "type": "ref/prompt", "name": NO_SUCH_PROMPT }),
        );
        params.insert(
            "argument".to_owned(),
            json!({ "name": "probe", "value": "" }),
        );
        let what = if capabilities.get("completions").is_some() {
            "completion/complete for a prompt the server does not list"
        } else {
            "completion/complete, which the server did not declare"
        };
        self.probe(what, "completion/complete", params).await?;
        if !legacy && capabilities.get("tools").is_some() {
            let mut params = Map::new();
            params.insert("cursor".to_owned(), json!(1));
            self.probe(
                "tools/list with a cursor that is not a string",
                "tools/list",
                params,
            )
            .await?;
        }
        Ok(())
    }
}
