// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reference MCP server exercising every protocol capability — in progress.
//!
//! The server implementation lands at roadmap milestone M2 (see
//! `docs/plan/06-roadmap.md` in the repository). What ships today is the part that must
//! exist *before* any listener does: [`policy`], the default-secure HTTP transport
//! policy that closes the CVE-2026-42559 class (DNS rebinding via unvalidated `Host` /
//! `Origin` headers) by construction. Building the policy first — tested,
//! property-checked, and mutation-gated — means the eventual server cannot be wired up
//! insecurely by accident.
//!
//! This crate makes no claims beyond its contents: there is no server here yet, and its
//! README says the same.

pub mod policy;
