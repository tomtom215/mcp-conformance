// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The feature sweep: everything a server serves that is not a tool.
//!
//! [`run`](crate::run::run) sweeps the tool list, which is most of what the
//! official suite's scenarios do and a fraction of what the specification
//! binds. A recording of tool calls alone leaves the prompts, resources,
//! templates, completion and error-code clauses with no traffic to judge —
//! they report *not observed*, correctly, and a corpus of such recordings
//! evidences far less of the surface than its pass count suggests.
//!
//! Everything here is **conforming client behaviour**: it lists what the
//! server advertises and asks for what the listing named, so a finding in the
//! recording is the server's. The one deliberate miss — a read of a URI the
//! catalog does not contain — is conforming too: asking for something absent
//! is a client's right, and the *error* it draws is the only way a recording
//! can carry the error-shape and error-code clauses at all. That step found a
//! real defect the first time it ran (the server answered `-32002`, which
//! `2026-07-28` withdrew), which is the argument for it in one line.
//!
//! Discovery-driven rather than scripted, on purpose. A fixed list of prompt
//! names would record this server's fixtures; asking `prompts/list` first
//! records whatever the server under test publishes, which is what makes the
//! sweep worth pointing at an implementation that is not ours.

use rmcp::model::{GetPromptRequestParams, Prompt, ReadResourceRequestParams};
use rmcp::service::{Peer, RoleClient};
use serde_json::{Map, Value};

#[cfg(test)]
mod tests;

/// A URI no catalog should contain, used to draw one error response.
///
/// `test://` is the scheme the conformance fixtures use, so this reads as a
/// resource of the server under test rather than as a malformed URI: the
/// clause being exercised is "what does a server answer for a resource it does
/// not have", not "what does it do with nonsense".
const ABSENT_URI: &str = "test://no-such-resource";

/// One step of the sweep, as observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepStep {
    /// The request method, plus its subject where there is one
    /// (`prompts/get test_simple_prompt`).
    pub what: String,
    /// A one-line summary of the answer, or the error it drew. An error is not
    /// a failure of the sweep: the read of an absent URI is here to produce
    /// one.
    pub outcome: Result<String, String>,
}

/// What the sweep drove, in order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SweepReport {
    /// Every step attempted, in execution order.
    pub steps: Vec<SweepStep>,
}

impl SweepReport {
    /// How many steps drew an error response.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.outcome.is_err())
            .count()
    }

    fn record(&mut self, what: impl Into<String>, outcome: Result<String, String>) {
        self.steps.push(SweepStep {
            what: what.into(),
            outcome,
        });
    }
}

/// Drives the whole non-tool surface and reports what it found.
///
/// Never returns `Err`: a sweep is a *recording* pass, and a server that
/// refuses `prompts/list` has told the trace something worth keeping. Each
/// step's outcome is recorded and the sweep continues, so one unimplemented
/// feature cannot truncate the evidence for the others.
///
/// Resources come first because prompts need them: a prompt argument asking
/// for a resource URI must be given one the server actually has, or the
/// content it embeds is a URI nobody published — which the trace would then
/// report as the *server's* malformed resource rather than the client's
/// invented argument.
pub async fn run(peer: &Peer<RoleClient>) -> SweepReport {
    let mut report = SweepReport::default();
    let resource = sweep_resources(peer, &mut report).await;
    let prompts = sweep_prompts(peer, resource.as_deref(), &mut report).await;
    sweep_completion(peer, prompts.as_slice(), &mut report).await;
    report
}

/// `resources/list`, `resources/templates/list`, a read of everything named —
/// and, last, one read that is meant to fail.
///
/// Returns the first listed URI, for the prompt arguments that need one.
async fn sweep_resources(peer: &Peer<RoleClient>, report: &mut SweepReport) -> Option<String> {
    let listed = match peer.list_resources(None).await {
        Ok(result) => {
            report.record(
                "resources/list",
                Ok(format!("{} resource(s)", result.resources.len())),
            );
            result.resources
        }
        Err(error) => {
            report.record("resources/list", Err(error.to_string()));
            Vec::new()
        }
    };
    for resource in &listed {
        read(peer, &resource.uri, report).await;
    }
    match peer.list_resource_templates(None).await {
        Ok(result) => {
            report.record(
                "resources/templates/list",
                Ok(format!("{} template(s)", result.resource_templates.len())),
            );
            for template in &result.resource_templates {
                if let Some(uri) = substitute(&template.uri_template) {
                    read(peer, &uri, report).await;
                }
            }
        }
        Err(error) => report.record("resources/templates/list", Err(error.to_string())),
    }
    // Deliberately last: the error response is the point, and a recording is
    // easier to read when the one expected failure sits at the end.
    read(peer, ABSENT_URI, report).await;
    listed.first().map(|resource| resource.uri.clone())
}

/// One `resources/read`, recorded either way.
async fn read(peer: &Peer<RoleClient>, uri: &str, report: &mut SweepReport) {
    let outcome = peer
        .read_resource(ReadResourceRequestParams::new(uri))
        .await
        .map(|result| format!("{} content block(s)", result.contents.len()))
        .map_err(|error| error.to_string());
    report.record(format!("resources/read {uri}"), outcome);
}

/// A concrete URI for an RFC 6570 template, by filling every `{var}` with `1`.
///
/// `None` when the template carries no variable — there is nothing to
/// substitute, and reading it would just repeat a `resources/list` entry.
/// Only the simple `{var}` form is filled: the operator-prefixed forms
/// (`{+var}`, `{#var}`, `{?var}`) change what the value means, and guessing at
/// one would synthesize a URI the server never meant to publish.
fn substitute(template: &str) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut substituted = false;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}')?;
        let variable = &rest[open + 1..open + close];
        if variable.is_empty()
            || !variable
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return None;
        }
        out.push_str(&rest[..open]);
        out.push('1');
        rest = &rest[open + close + 1..];
        substituted = true;
    }
    out.push_str(rest);
    substituted.then_some(out)
}

/// `prompts/list`, then `prompts/get` for each prompt it named.
///
/// Returns the listing so the completion step can address a real prompt
/// argument rather than a guessed one.
async fn sweep_prompts(
    peer: &Peer<RoleClient>,
    resource: Option<&str>,
    report: &mut SweepReport,
) -> Vec<Prompt> {
    let listed = match peer.list_prompts(None).await {
        Ok(result) => result.prompts,
        Err(error) => {
            report.record("prompts/list", Err(error.to_string()));
            return Vec::new();
        }
    };
    report.record("prompts/list", Ok(format!("{} prompt(s)", listed.len())));
    for prompt in &listed {
        let mut params = GetPromptRequestParams::new(prompt.name.clone());
        params.arguments = Some(prompt_arguments(prompt, resource));
        let outcome = peer
            .get_prompt(params)
            .await
            .map(|result| format!("{} message(s)", result.messages.len()))
            .map_err(|error| error.to_string());
        report.record(format!("prompts/get {}", prompt.name), outcome);
    }
    listed
}

/// Values for a prompt's declared arguments.
///
/// Every argument gets one, required or not: an optional argument is part of
/// the surface too, and supplying it exercises a path the minimal call does
/// not. The value is a string because prompt arguments are untyped in the
/// protocol — the schema that would type them belongs to tools, not prompts.
fn prompt_arguments(prompt: &Prompt, resource: Option<&str>) -> Map<String, Value> {
    prompt
        .arguments
        .iter()
        .flatten()
        .map(|argument| {
            (
                argument.name.clone(),
                Value::String(argument_value(&argument.name, resource)),
            )
        })
        .collect()
}

/// The value supplied for one prompt argument.
///
/// An argument whose name says it names a resource gets a URI the server
/// listed, never a placeholder. The everything server's
/// `test_prompt_with_embedded_resource` embeds this value verbatim, so
/// `"probe"` there would produce an embedded resource whose URI has no RFC
/// 3986 scheme — a `RES-004` finding manufactured by the client, reported
/// against the server. A discovered URI is both realistic and sound.
fn argument_value(name: &str, resource: Option<&str>) -> String {
    match resource {
        Some(uri) if name.to_ascii_lowercase().contains("resource") => uri.to_owned(),
        _ => "probe".to_owned(),
    }
}

/// `completion/complete` against a real prompt argument.
///
/// Addressed at the first prompt that declares one, because the request names
/// both the prompt and the argument: completing an argument the prompt does
/// not have is a different clause's subject (an error), and this step records
/// the success path.
async fn sweep_completion(peer: &Peer<RoleClient>, prompts: &[Prompt], report: &mut SweepReport) {
    let Some((prompt, argument)) = prompts.iter().find_map(|prompt| {
        let argument = prompt.arguments.iter().flatten().next()?;
        Some((prompt, argument))
    }) else {
        return; // No prompt declares an argument; nothing to complete.
    };
    let outcome = peer
        .complete_prompt_argument(prompt.name.clone(), argument.name.clone(), "", None)
        .await
        .map(|info| format!("{} value(s)", info.values.len()))
        .map_err(|error| error.to_string());
    report.record(
        format!("completion/complete {}.{}", prompt.name, argument.name),
        outcome,
    );
}
