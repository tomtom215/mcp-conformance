// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reference MCP host: a deterministic, scriptable client-side system under
//! test (roadmap M3, ADR-0009).
//!
//! Three pieces ship today, each pure or transport-agnostic and tested
//! in-process against `mcp-everything-server`:
//!
//! - [`script`] — every behavior a model or user would supply, as data: the
//!   sampling reply, the elicitation policy (including SEP-1034 defaults and
//!   URL-mode consent), and the roots list. Zero model-provider network use
//!   holds by construction.
//! - [`handler`] — the [`rmcp::ClientHandler`] answering from a script, with
//!   an event log making every server-initiated interaction assertable, and
//!   the URL-elicitation pending-id set enforcing the spec's "ignore unknown
//!   or already-completed ids" client MUST.
//! - [`run`] — the bounded tool-use loop: a deterministic call policy under
//!   an explicit stop-condition lattice (cancellation, turn limit, error
//!   budget, completion), every variant a tested stop reason.
//! - [`retry`] — the deterministic backoff policy (shipped since v0.1.0);
//!   the SSE `retry` field is a server-named delay, which is exactly
//!   [`retry::RetryPolicy::delay_honoring_retry_after`].
//!
//! The binary, the child-process and streamable-HTTP transports, and the
//! official-suite client-scenario wiring land in the next M3 slice; this
//! crate's README states exactly what is and is not here yet.

pub mod handler;
pub mod retry;
pub mod run;
pub mod script;
