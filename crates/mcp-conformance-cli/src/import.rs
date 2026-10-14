// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Recordings made by other tools, as traces.
//!
//! Two importers, each documented with the format it reads and — as the
//! [`Imported::not_recorded`] list a library entry keeps — what that format
//! does not record, so the trace cannot show it. Nothing is invented to fill
//! the gap: a fact the source did not record is absent from the trace, and
//! the validator reports the clauses that needed it as not observed.
//!
//! - [`mcpsnoop`]: the JSON Lines capture `mcpsnoop` writes (the reverse of
//!   the conversion `cargo xtask mcpsnoop` makes).
//! - [`inspector`]: the protocol export of the MCP Inspector's web client.
//!
//! A trace this toolkit recorded imports as itself, once it parses.
//!
//! ```
//! use mcp_conformance_cli::import::{self, Format};
//!
//! let capture = r#"{"session_id":"s","seq":1,"ts":"2026-10-13T10:00:00Z","direction":"meta","transport":"stdio","raw":{"command":["server"]}}
//! {"session_id":"s","seq":2,"ts":"2026-10-13T10:00:00.001Z","direction":"c2s","transport":"stdio","raw":{"jsonrpc":"2.0","id":1,"method":"ping"}}
//! "#;
//! assert_eq!(import::detect(capture), Some(Format::Mcpsnoop));
//! let imported = import::import(capture, Format::Mcpsnoop, &import::Options::default())?;
//! assert!(imported.trace.contains(r#""method":"ping""#));
//! assert!(!imported.not_recorded.is_empty());
//! # Ok::<(), import::ImportError>(())
//! ```

use core::fmt;

use mcp_conformance_core::trace::{TraceEvent, TransportKind};
use mcp_trace_validator::reader::{self, Limits};
use serde_json::Value;

pub mod inspector;
pub mod mcpsnoop;
mod time;

/// A source format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// A trace in this toolkit's own format.
    Trace,
    /// An `mcpsnoop` capture.
    Mcpsnoop,
    /// An MCP Inspector protocol export.
    Inspector,
}

impl Format {
    /// The format's name, as a library sidecar records it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Mcpsnoop => "mcpsnoop",
            Self::Inspector => "inspector",
        }
    }

    /// The format a name names.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        [Self::Trace, Self::Mcpsnoop, Self::Inspector]
            .into_iter()
            .find(|format| format.name() == name)
    }
}

/// How to import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// The transport the session used, for a format that does not record
    /// it (the Inspector's export). Ignored by the formats that do.
    pub transport: Option<TransportKind>,
}

impl Options {
    /// These options, stating the session's transport.
    #[must_use]
    pub const fn transport(mut self, transport: TransportKind) -> Self {
        self.transport = Some(transport);
        self
    }
}

/// A recording, as a trace.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Imported {
    /// The trace, JSON Lines.
    pub trace: String,
    /// What the source format does not record, so the trace cannot show.
    pub not_recorded: Vec<String>,
    /// When the session was recorded (RFC 3339), when the source says.
    pub recorded_at: Option<String>,
}

/// Why a document could not be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError(pub String);

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImportError {}

/// The format `text` is in, read from its first record; `None` when it is
/// none of the three.
#[must_use]
pub fn detect(text: &str) -> Option<Format> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('[') {
        let entries: Value = serde_json::from_str(trimmed).ok()?;
        let first = entries.as_array()?.first()?;
        return (first.get("origin").is_some() && first.get("message").is_some())
            .then_some(Format::Inspector);
    }
    let first: Value = serde_json::from_str(trimmed.lines().next()?).ok()?;
    if first.get("session_id").is_some() && first.get("direction").is_some() {
        Some(Format::Mcpsnoop)
    } else if first.get("seq").is_some() && first.get("kind").is_some() {
        Some(Format::Trace)
    } else {
        None
    }
}

/// Imports `text`, in `format`.
///
/// # Errors
///
/// [`ImportError`] when `text` is not a document of that format, or the
/// format needs an option that is not given (the Inspector's transport).
pub fn import(text: &str, format: Format, options: &Options) -> Result<Imported, ImportError> {
    match format {
        Format::Trace => {
            let events = reader::parse_trace(text, &Limits::default())
                .map_err(|error| ImportError(format!("not a trace: {error}")))?;
            Ok(Imported {
                trace: text.to_owned(),
                not_recorded: Vec::new(),
                recorded_at: events.first().and_then(|event| event.ts.clone()),
            })
        }
        Format::Mcpsnoop => mcpsnoop::import(text),
        Format::Inspector => {
            let transport = options.transport.ok_or_else(|| {
                ImportError(
                    "the Inspector's protocol export does not record which transport the session \
                     used; state it (stdio or streamable-http)"
                        .to_owned(),
                )
            })?;
            inspector::import(text, transport)
        }
    }
}

/// `events` as a trace document, renumbered from 0 in order.
fn write(events: Vec<TraceEvent>) -> String {
    let mut out = String::new();
    for (seq, mut event) in events.into_iter().enumerate() {
        event.seq = seq as u64;
        out.push_str(&serde_json::to_string(&event).unwrap_or_default());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;
