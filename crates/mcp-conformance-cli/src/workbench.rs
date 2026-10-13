// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-conformance serve`, the workbench (ADR-0021, decision 4), and
//! `mcp-conformance library`, the same library from the command line.
//!
//! | Module | What it is |
//! |--------|------------|
//! | [`library`], [`meta`] | the directory of traces and their sidecars |
//! | [`entry`], [`state`] | the library in memory, its verdicts from the validator |
//! | [`query`], [`search`] | the search box's language and index |
//! | [`view`], [`diff`], [`stats`] | what the session, diff and dashboard views show |
//! | [`redact`], [`tar`], [`ops`] | administration: redacted copies, exports, imports, retention |
//! | [`guard`], [`api`], [`assets`], `server` | the local web server and its defenses |

pub(crate) mod api;
pub(crate) mod assets;
pub(crate) mod clock;
pub(crate) mod diff;
pub(crate) mod entry;
pub(crate) mod guard;
pub(crate) mod library;
pub(crate) mod meta;
pub(crate) mod ops;
pub(crate) mod query;
pub(crate) mod redact;
pub(crate) mod search;
pub(crate) mod server;
pub(crate) mod state;
pub(crate) mod stats;
pub(crate) mod tar;
pub(crate) mod view;

#[cfg(test)]
pub(crate) mod fixture;
