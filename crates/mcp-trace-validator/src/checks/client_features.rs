// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the client-feature pages: `client/elicitation`, `client/sampling`
//! and `client/roots`, at both revisions.
//!
//! The pages are about features the *client* offers, which is why they were once
//! excluded wholesale. But they bind the server as much as the client: what it
//! may ask for, in which mode, with which parameters, and only of a client that
//! declared the capability. Under MRTR (`2026-07-28`) those questions travel
//! inside the server's own result, and they bind it no less.
//!
//! The pages' sentences are largely the same at both revisions, and so is every
//! check here: [`asks`] presents a `2025-11-25` server-to-client request and a
//! `2026-07-28` `inputRequests` entry as one kind of thing, and the client's
//! response or retry likewise. The few rules one revision states alone — the
//! `2025-11-25` URL mode bookkeeping in [`url_flow`], the client error codes,
//! `2026-07-28`'s message shape — are separate checks named only by that
//! revision's registry.

mod asks;
pub(super) mod elicitation;
pub(super) mod roots;
pub(super) mod sampling;
mod uri;
pub(super) mod url_flow;

#[cfg(test)]
mod testkit;
