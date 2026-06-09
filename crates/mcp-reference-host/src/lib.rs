// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reference MCP host and agent loop — in progress.
//!
//! The host runtime lands at roadmap milestone M3 (see `docs/plan/06-roadmap.md` in the
//! repository). What ships today is [`retry`]: the deterministic backoff policy the
//! host's transport layer will be built on. It is pure arithmetic — jitter enters as a
//! caller-supplied unit value, `Retry-After` as a caller-parsed duration — so every
//! delay the eventual host will ever compute is testable to the exact value today.
//!
//! This crate makes no claims beyond its contents: there is no host here yet, and its
//! README says the same.

pub mod retry;
