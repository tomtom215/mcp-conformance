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
//!   [`retry::RetryPolicy::delay_honoring_retry_after`] — and `resume` is
//!   where that becomes load-bearing.
//! - [`capture`] — host-side trace capture: a `Transport` wrapper recording
//!   every message as a validator-ready trace, redaction by construction
//!   (the seam never sees headers).
//! - [`connect`] — the two real transports, from rmcp's official client
//!   features: child-process stdio (feature `proc`) and streamable HTTP
//!   (feature `http`).
//! - [`scenario`] — the pinned suite's client scenarios as plans; one table,
//!   governed by the suite pin (ADR-0009).
//! - `resume` (feature `http`) — the compliant SSE-resumption dance rmcp
//!   1.7's transport cannot perform (measured; ADR-0009 §Amendment).
//!
//! The binary (`cli` feature) maps `MCP_CONFORMANCE_SCENARIO` to a plan and
//! exits clean on success — the pinned suite's client-SUT contract.

pub mod capture;
pub mod connect;
pub mod handler;
#[cfg(feature = "http")]
pub mod resume;
pub mod retry;
pub mod run;
pub mod scenario;
pub mod script;
