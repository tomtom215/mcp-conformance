// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Record any Model Context Protocol session as a trace `mcp-trace-validator` can
//! judge.
//!
//! Two recorders, both transparent to the session they record:
//!
//! - [`stdio`] runs the server as a child process and sits on its stdin/stdout.
//!   A client configured to launch `mcp-trace-capture stdio -- <server>` in place of
//!   `<server>` works exactly as before.
//! - [`http`] is a reverse proxy in front of a streamable-HTTP server, SSE included.
//!
//! Both write the same JSON Lines trace ([`mcp_conformance_core::trace`]), record a
//! message before forwarding the bytes that complete it — so `seq` order is causal —
//! and never alter traffic to fit the trace: what cannot be recorded (non-JSON output,
//! a message over the size limit) is forwarded intact and counted. Neither depends on
//! any MCP SDK, so the trace describes the bytes on the wire, not one SDK's reading of
//! them.

pub mod framing;
pub mod headers;
pub mod http;
pub mod recorder;
pub mod stdio;

pub use recorder::{Recorder, Summary};

/// The default largest message recorded, in bytes (64 MiB).
pub const DEFAULT_MAX_MESSAGE: usize = 64 * 1024 * 1024;
