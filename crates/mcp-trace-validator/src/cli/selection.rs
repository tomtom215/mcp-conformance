// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `--area` and `--level`: which clauses a command names, parsed once for the
//! two commands that take them.
//!
//! `requirements` selects by them what it lists; `validate` selects by them what
//! its human report lists, and nothing it decides (ADR-0025). Sharing the
//! parser is what makes `--area TOOL --level must` mean the same clauses in
//! both.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use clap::ValueEnum;
use mcp_conformance_core::requirement::Level;

/// An RFC 2119 level, as `--level` spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LevelArg {
    Must,
    MustNot,
    Should,
    ShouldNot,
    May,
}

impl LevelArg {
    pub(crate) const fn level(self) -> Level {
        match self {
            Self::Must => Level::Must,
            Self::MustNot => Level::MustNot,
            Self::Should => Level::Should,
            Self::ShouldNot => Level::ShouldNot,
            Self::May => Level::May,
        }
    }
}

/// The `--area` and `--level` flags.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct Selection {
    /// Only clauses in this area — the ID's prefix, as `LIFE` in `LIFE-001`;
    /// repeatable, or comma-separated (`--area BASE,TOOL`). `requirements`
    /// lists those; `validate` lists those in its human report, and its totals,
    /// verdict and exit status still count every clause.
    #[arg(long, value_name = "AREA", value_delimiter = ',')]
    pub(crate) area: Vec<String>,
    /// Only clauses at this RFC 2119 level (`must`, `must-not`, `should`,
    /// `should-not`, `may`; any case); repeatable, or comma-separated
    /// (`--level must,should`). Lists only, as `--area` does.
    #[arg(long, value_enum, ignore_case = true, value_delimiter = ',')]
    pub(crate) level: Vec<LevelArg>,
}

impl Selection {
    /// The areas named, in the registry's upper case.
    pub(crate) fn areas(&self) -> Vec<String> {
        self.area
            .iter()
            .map(|area| area.to_ascii_uppercase())
            .collect()
    }

    /// The levels named, as the keyword a report row carries (`MUST NOT`).
    pub(crate) fn level_keywords(&self) -> Vec<String> {
        self.level
            .iter()
            .map(|level| level.level().keyword().to_owned())
            .collect()
    }
}
