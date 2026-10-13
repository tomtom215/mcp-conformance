// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `client/sampling`: tool use in sampling, the deprecated `includeContext`
//! values, message shape, and what the client's answer shows about `toolChoice`.
//!
//! The message rules judge the `messages` a server sends. In a tool loop each
//! further round's request carries the conversation so far — the model's tool
//! uses as assistant messages and the server's results as user messages — so
//! "every assistant message containing `ToolUseContent` blocks MUST be followed
//! by a user message that consists entirely of `ToolResultContent` blocks" is a
//! property of one request's `messages`, visible whole.

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::FindingSink;
use super::asks::{Ask, answers_for, asks_for};
use super::elicitation::declared_where_answered;
use crate::context::TraceContext;

/// The sampling method.
const SAMPLE: &str = "sampling/createMessage";

/// The `includeContext` values the page deprecates.
const DEPRECATED_CONTEXT: &[&str] = &["thisServer", "allServers"];

/// A message's content blocks: `content` is one block or an array of them.
fn blocks(message: &Value) -> Vec<&Value> {
    match message.get("content") {
        Some(Value::Array(blocks)) => blocks.iter().collect(),
        Some(block) => vec![block],
        None => Vec::new(),
    }
}

/// A block's `type`.
fn block_type(block: &Value) -> Option<&str> {
    block.get("type").and_then(Value::as_str)
}

/// A message's `role`.
fn role(message: &Value) -> Option<&str> {
    message.get("role").and_then(Value::as_str)
}

/// The `messages` an ask sends.
fn messages<'a>(ask: &Ask<'a>) -> Vec<&'a Value> {
    ask.param("messages")
        .and_then(Value::as_array)
        .map(|messages| messages.iter().collect())
        .unwrap_or_default()
}

/// The deprecated `includeContext` value an ask uses, if it uses one.
fn deprecated_context<'a>(ask: &Ask<'a>) -> Option<&'a str> {
    ask.param("includeContext")
        .and_then(Value::as_str)
        .filter(|value| DEPRECATED_CONTEXT.contains(value))
}

/// The index of each user message that mixes tool results with other content.
fn mixed_tool_results(messages: &[&Value]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| role(message) == Some("user"))
        .filter(|(_, message)| {
            let types: Vec<Option<&str>> = blocks(message).into_iter().map(block_type).collect();
            types.contains(&Some("tool_result")) && types.iter().any(|t| *t != Some("tool_result"))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Why the assistant message at `index` — one carrying tool uses — is not
/// followed by a user message of tool results answering every one of them.
fn unanswered_tool_use(messages: &[&Value], index: usize) -> Option<String> {
    let uses: BTreeSet<&str> = blocks(messages[index])
        .into_iter()
        .filter(|block| block_type(block) == Some("tool_use"))
        .filter_map(|block| block.get("id").and_then(Value::as_str))
        .collect();
    let Some(next) = messages.get(index + 1) else {
        return Some("is the last message, with no tool results after it".to_owned());
    };
    let results = blocks(next);
    if role(next) != Some("user")
        || results.is_empty()
        || results
            .iter()
            .any(|block| block_type(block) != Some("tool_result"))
    {
        return Some(
            "is followed by a message that is not a user message of tool results only".to_owned(),
        );
    }
    let answered: BTreeSet<&str> = results
        .iter()
        .filter_map(|block| block.get("toolUseId").and_then(Value::as_str))
        .collect();
    let missing: Vec<&str> = uses.difference(&answered).copied().collect();
    (!missing.is_empty()).then(|| {
        format!(
            "has tool use(s) {} that the next message's tool results do not answer",
            missing
                .iter()
                .map(|id| format!("{id:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// The indices of assistant messages carrying tool uses.
fn tool_use_messages(messages: &[&Value]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            role(message) == Some("assistant")
                && blocks(message)
                    .into_iter()
                    .any(|block| block_type(block) == Some("tool_use"))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Whether an ask's messages break either tool-message rule.
fn malformed_tool_messages(ask: &Ask<'_>) -> bool {
    let messages = messages(ask);
    !mixed_tool_results(&messages).is_empty()
        || tool_use_messages(&messages)
            .into_iter()
            .any(|index| unanswered_tool_use(&messages, index).is_some())
}

/// "Clients that support sampling MUST declare the `sampling` capability".
pub(in crate::checks) fn client_declares_capability(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    declared_where_answered(context, sink, SAMPLE, "sampling");
}

/// "Servers MUST NOT send tool-enabled sampling requests to Clients that have
/// not declared support for tool use via the `sampling.tools` capability."
///
/// Tool-enabled means carrying `tools`, the array the page's "Tools in
/// Sampling" names as the request's tools.
pub(in crate::checks) fn tools_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, SAMPLE) {
        if ask.param("tools").is_none() {
            continue;
        }
        let Some(declared) = ask.declared.declares(&["sampling", "tools"]) else {
            continue;
        };
        sink.examined();
        if !declared {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} carries `tools`, but {} does not declare `sampling.tools`",
                    ask.label(),
                    ask.declaration()
                ),
            );
        }
    }
}

/// "Servers SHOULD avoid using these values" — `includeContext` of
/// `thisServer` or `allServers`.
pub(in crate::checks) fn include_context_avoided(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for ask in asks_for(context, SAMPLE) {
        sink.examined();
        if let Some(value) = deprecated_context(&ask) {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} uses the deprecated `includeContext` value {value:?}",
                    ask.label()
                ),
            );
        }
    }
}

/// "… and SHOULD NOT use them unless the client declares `sampling.context`
/// capability."
pub(in crate::checks) fn include_context_declared(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for ask in asks_for(context, SAMPLE) {
        let Some(value) = deprecated_context(&ask) else {
            continue;
        };
        let Some(declared) = ask.declared.declares(&["sampling", "context"]) else {
            continue;
        };
        sink.examined();
        if !declared {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} uses `includeContext` {value:?}, but {} does not declare \
                     `sampling.context`",
                    ask.label(),
                    ask.declaration()
                ),
            );
        }
    }
}

/// "When a user message contains tool results (type: `"tool_result"`), it MUST
/// contain ONLY tool results."
pub(in crate::checks) fn tool_results_only(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, SAMPLE) {
        let messages = messages(&ask);
        for message in &messages {
            let carries_results = role(message) == Some("user")
                && blocks(message)
                    .into_iter()
                    .any(|block| block_type(block) == Some("tool_result"));
            if carries_results {
                sink.examined();
            }
        }
        for index in mixed_tool_results(&messages) {
            sink.push(
                Some(ask.seq),
                format!(
                    "{}'s `messages[{index}]` mixes tool results with other content",
                    ask.label()
                ),
            );
        }
    }
}

/// "… every assistant message containing `ToolUseContent` blocks MUST be
/// followed by a user message that consists entirely of `ToolResultContent`
/// blocks, with each tool use … matched by a corresponding tool result …,
/// before any other message."
pub(in crate::checks) fn tool_use_answered(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, SAMPLE) {
        let messages = messages(&ask);
        for index in tool_use_messages(&messages) {
            sink.examined();
            if let Some(why) = unanswered_tool_use(&messages, index) {
                sink.push(
                    Some(ask.seq),
                    format!("{}'s `messages[{index}]` {why}", ask.label()),
                );
            }
        }
    }
}

/// Why a sampling message is not the shape the page requires.
fn shape_defect(message: &Value) -> Option<&'static str> {
    if !matches!(role(message), Some("user" | "assistant")) {
        Some("has no `role` of \"user\" or \"assistant\"")
    } else if message.get("content").is_none() {
        Some("has no `content`")
    } else {
        None
    }
}

/// `2026-07-28` "Sampling messages MUST contain a `role` field of `"user"` or
/// `"assistant"`; and a `content` field": every message a server sends, and
/// every result a client returns, which is itself a sampling message.
pub(in crate::checks) fn message_shape(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, SAMPLE) {
        for (index, message) in messages(&ask).into_iter().enumerate() {
            sink.examined();
            if let Some(why) = shape_defect(message) {
                sink.push(
                    Some(ask.seq),
                    format!("{}'s `messages[{index}]` {why}", ask.label()),
                );
            }
        }
    }
    for answer in answers_for(context, SAMPLE) {
        let Some(result) = answer.result else {
            continue;
        };
        sink.examined();
        if let Some(why) = shape_defect(result) {
            sink.push(
                Some(answer.seq),
                format!("the client's answer to {} {why}", answer.ask.label()),
            );
        }
    }
}

/// Reports answers to asks with `toolChoice.mode` of `mode` whose content does
/// (`forbidden`) or does not (`!forbidden`) use a tool.
fn tool_choice_honoured(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    mode: &str,
    forbidden: bool,
) {
    for answer in answers_for(context, SAMPLE) {
        let chosen = answer
            .ask
            .param("toolChoice")
            .and_then(|choice| choice.get("mode"))
            .and_then(Value::as_str);
        let Some(result) = answer.result.filter(|_| chosen == Some(mode)) else {
            continue;
        };
        sink.examined();
        let used = blocks(result)
            .into_iter()
            .any(|block| block_type(block) == Some("tool_use"));
        if used == forbidden {
            sink.push(
                Some(answer.seq),
                format!(
                    "{} set `toolChoice` mode {mode:?}, and the client's answer {}",
                    answer.ask.label(),
                    if forbidden {
                        "uses a tool"
                    } else {
                        "uses no tool"
                    }
                ),
            );
        }
    }
}

/// "`{mode: "required"}`: Model MUST use at least one tool before completing".
pub(in crate::checks) fn tool_choice_required_honoured(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    tool_choice_honoured(context, sink, "required", false);
}

/// "`{mode: "none"}`: Model MUST NOT use any tools".
pub(in crate::checks) fn tool_choice_none_honoured(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    tool_choice_honoured(context, sink, "none", true);
}

/// `2025-11-25` "Clients SHOULD return errors for common failure cases: Tool
/// result missing in request: `-32602` (Invalid params); Tool results mixed
/// with other content: `-32602` (Invalid params)".
///
/// The subject is a direct request whose `messages` break either tool-message
/// rule — the two failure cases the rows name, judged by the same predicates as
/// the server-side clauses.
pub(in crate::checks) fn malformed_tool_messages_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for answer in answers_for(context, SAMPLE) {
        if answer.ask.is_mrtr() || !malformed_tool_messages(&answer.ask) {
            continue;
        }
        sink.examined();
        if answer.error_code != Some(-32602) {
            sink.push(
                Some(answer.seq),
                format!(
                    "the client answered {}, whose tool messages are malformed, with {} rather \
                     than the `-32602` error",
                    answer.ask.label(),
                    answer
                        .error_code
                        .map_or_else(|| "a result".to_owned(), |code| format!("error {code}"))
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
