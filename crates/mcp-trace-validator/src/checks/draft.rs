// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2026-07-28` revision.
//!
//! Referenced by the `2026-07-28` registry entries and, like every check, run
//! against whatever registry the caller projects — a check is a pure function of
//! a trace, not of a revision. They live together here because they arrived
//! together with that revision's first extracted areas, and because splitting
//! them out keeps the `2025-11-25` modules reviewable in isolation.

pub(super) mod envelope;
pub(super) mod meta;
