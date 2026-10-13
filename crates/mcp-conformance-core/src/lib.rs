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
//! - [`requirement`] — the requirement registry: normative spec clauses as records
//!   carrying stable IDs, an RFC 2119 [`requirement::Level`], verbatim source quotes,
//!   and a check-or-documented-exclusion verification mapping. The mapping shape follows
//!   SEP-2484's traceability format. The format has a `MAY` level, but the built-in
//!   registries enter only MUST, MUST NOT, SHOULD and SHOULD NOT clauses: a `MAY`
//!   permits, so no trace can break it.
//! - [`capability`] — capability gates: the negotiated-capability paths that decide
//!   whether a gated requirement applies to a given session (ADR-0006).
//! - [`applies`] — revision ranges: the half-open `[introduced, removed)` interval that
//!   decides whether a requirement is in force at a given protocol revision (ADR-0006).
//! - [`message`] — structural classification of JSON-RPC 2.0 messages (request /
//!   notification / response) as MCP constrains them.
//! - [`trace`] — the recorded-trace event schema (JSON Lines of [`trace::TraceEvent`])
//!   that conformance traces are exchanged in.
//! - [`wire_schema`] — the protocol's own JSON Schema for each revision, vendored
//!   from the specification with the places it contradicts `schema.ts` recorded.
//! - [`canonical`] — deterministic canonical JSON serialization used wherever payloads
//!   are compared or hashed.
//!
//! # Built-in registries
//!
//! The registries for every revision this build judges are embedded in the crate.
//! [`requirement::RegistrySet::builtin`] loads them as one set — the starting point for
//! judging the current revision, `2026-07-28`, or several at once — and
//! [`requirement::RegistrySet::registry`] projects the set to one revision.
//! [`requirement::Registry::builtin_2025_11_25`] loads the `2025-11-25` registry alone.
//! Every quote was taken verbatim from the published specification text, and each
//! revision's registry covers every normative clause of the pages its
//! `registry/<revision>/sources.json` lists as in scope.
//!
//! # Example
//!
//! ```
//! use mcp_conformance_core::requirement::{RegistrySet, Verification};
//!
//! let set = RegistrySet::builtin().expect("embedded registries are valid");
//! let newest = set.latest().expect("the built-in set describes a revision");
//! let registry = set.registry(newest).expect("the set describes its newest revision");
//! assert_eq!(registry.revision().to_string(), "2026-07-28");
//!
//! // Every clause is either judged by checks or excluded with a written reason.
//! for requirement in registry.requirements() {
//!     match &requirement.verification {
//!         Verification::Checks { checks } => assert!(!checks.is_empty()),
//!         Verification::Excluded { exclusion } => assert!(!exclusion.is_empty()),
//!         _ => {}
//!     }
//! }
//! ```

pub mod applies;
pub mod canonical;
pub mod capability;
pub mod message;
pub mod requirement;
pub mod revision;
pub mod trace;
pub mod wire_schema;
