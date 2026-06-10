// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Core data model for Model Context Protocol (MCP) conformance tooling.
//!
//! This crate is the *spec as data*: it defines the machine-readable shapes that the rest
//! of the toolkit operates on, and deliberately performs **no I/O** — every function here
//! is a pure transformation, which is what makes the downstream validator deterministic
//! and trivially testable.
//!
//! # Modules
//!
//! - [`revision`] — dated protocol revisions (`2025-11-25`-style identifiers) with total
//!   ordering.
//! - [`requirement`] — the requirement registry: normative spec clauses (MUST / MUST NOT /
//!   SHOULD / SHOULD NOT / MAY) as records carrying stable IDs, verbatim source quotes,
//!   and a check-or-documented-exclusion verification mapping. The mapping shape follows
//!   SEP-2484's traceability format.
//! - [`capability`] — capability gates: the negotiated-capability paths that decide
//!   whether a gated requirement applies to a given session (ADR-0006).
//! - [`message`] — structural classification of JSON-RPC 2.0 messages (request /
//!   notification / response) as MCP constrains them.
//! - [`trace`] — the recorded-trace event schema (JSON Lines of [`trace::TraceEvent`])
//!   that conformance traces are exchanged in.
//! - [`canonical`] — deterministic canonical JSON serialization used wherever payloads
//!   are compared or hashed.
//!
//! # Built-in registry
//!
//! A seed requirement registry for protocol revision `2025-11-25` is embedded in the
//! crate and available via [`requirement::Registry::builtin_2025_11_25`]. Every quote in
//! it was taken verbatim from the published specification text. The registry grows
//! toward full coverage of the revision; its format is stable even while its contents
//! expand.
//!
//! # Example
//!
//! ```
//! use mcp_conformance_core::requirement::Registry;
//!
//! let registry = Registry::builtin_2025_11_25().expect("embedded registry is valid");
//! assert_eq!(registry.revision().to_string(), "2025-11-25");
//! assert!(registry.requirements().len() >= 16);
//! ```

pub mod canonical;
pub mod capability;
pub mod message;
pub mod requirement;
pub mod revision;
pub mod trace;
