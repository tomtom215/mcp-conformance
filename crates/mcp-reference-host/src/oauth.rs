// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! OAuth for the official suite's `auth/*` client scenarios (feature `oauth`,
//! the binary's `--oauth-conformance`).
//!
//! **A conformance harness mode, not a way to sign in.** No person is asked
//! anything: the host fetches the authorization URL itself and takes the code
//! from the redirect, which is consent given by nobody. That is acceptable
//! against the suite's throwaway authorization server on this machine and
//! nowhere else, so the one step that does it ([`consent`]) refuses any
//! authorization endpoint that is not a loopback address.
//!
//! Everything OAuth is rmcp's (`rmcp::transport::auth`, feature `auth`):
//! protected-resource and authorization-server metadata discovery, client
//! registration (pre-registered, Client ID Metadata Document, dynamic), PKCE
//! `S256`, the `state` check, the RFC 8707 `resource` parameter, the token
//! exchange and the scope arithmetic. What this module adds is the part rmcp
//! leaves to its caller:
//!
//! - [`context`] — which client identity the suite handed over in
//!   `MCP_CONFORMANCE_CONTEXT`.
//! - [`consent`] — the headless stand-in for the browser.
//! - [`flow`] — one authorization, start to stored token, under a budget.
//! - [`client`] — the `StreamableHttpClient` wrapper that reacts to a `401` or
//!   a `403 insufficient_scope` from the MCP server by running the flow and
//!   sending the request again.
//!
//! Tokens, codes and secrets are held in memory for the life of the process,
//! never written anywhere and never printed: no type here that holds one
//! has a `Debug` that shows it, and no error message carries one.

pub mod client;
pub mod consent;
pub mod context;
pub mod flow;

/// The redirect URI the host registers and asks the authorization server to
/// send the code to.
///
/// Nothing listens on it. The host reads the code off the `Location` header of
/// the authorization response instead of following it, so the address only has
/// to be one the authorization server accepts for a native client.
pub const REDIRECT_URI: &str = "http://localhost:3000/callback";

/// The Client ID Metadata Document URL the host presents when the
/// authorization server says it accepts one (SEP-991).
///
/// The suite's own value: its `auth/basic-cimd` scenario checks that the
/// `client_id` it receives is exactly this URL, and nothing is ever fetched
/// from it.
pub const CLIENT_METADATA_URL: &str = "https://conformance-test.local/client-metadata.json";

/// The name the host registers under when it has to register dynamically.
pub const CLIENT_NAME: &str = "mcp-reference-host";

/// How many authorization requests one session may make, the first included.
///
/// The specification asks for a retry limit so that a server answering every
/// attempt with `403 insufficient_scope` cannot keep a client authorizing for
/// ever. Three is one sign-in and two escalations, which is the most the
/// suite's `auth/scope-retry-limit` scenario accepts.
pub const AUTHORIZATION_BUDGET: u32 = 3;
