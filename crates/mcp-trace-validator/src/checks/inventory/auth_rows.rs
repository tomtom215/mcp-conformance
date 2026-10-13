// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The authorization pages' rows of the check inventory, shared by both
//! revisions (see `checks::auth`).

use super::Check;
use crate::checks::auth;

macro_rules! check {
    ($id:literal, $run:path) => {
        Check { id: $id, run: $run }
    };
}

/// The rows, in stable order.
pub(super) const ROWS: &[Check] = &[
    check!("auth.challenge-scope", auth::challenge_scope),
    check!(
        "auth.insufficient-scope-response",
        auth::insufficient_scope_response
    ),
    check!(
        "auth.insufficient-scope-names-scopes",
        auth::insufficient_scope_names_scopes
    ),
    check!("auth.error-status", auth::error_status),
    check!("auth.invalid-token-401", auth::invalid_token_401),
    check!(
        "auth.no-offline-access-challenge",
        auth::no_offline_access_challenge
    ),
    check!("auth.single-challenge", auth::single_challenge),
];
