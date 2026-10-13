// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `requirements` subcommand: print a registry, part of one, or single
//! requirements in full.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fmt::Write as _;
use std::path::PathBuf;

use clap::ValueEnum;
use mcp_conformance_core::requirement::{Actor, Level, Registry, Requirement, Verification};
use mcp_conformance_core::revision::ProtocolRevision;
use serde::Serialize;

use crate::emit::emit;
use crate::{EXIT_OK, EXIT_USAGE, load_registry, load_registry_set};

/// The `requirements` subcommand's arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Requirement IDs to show in full (`LIFE-001`): the level, the party it
    /// binds, the clause and its link, and the checks that judge it or the
    /// reason none can. Without IDs, every requirement is listed one per line.
    #[arg(value_name = "ID")]
    ids: Vec<String>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
    /// Path to a registry JSON document, used instead of the built-in one.
    /// Mutually exclusive with `--revision`.
    #[arg(long)]
    registry: Option<PathBuf>,
    /// Which built-in revision to print; defaults to the newest.
    #[arg(long, value_name = "YYYY-MM-DD", conflicts_with = "registry")]
    revision: Option<String>,
    /// Only requirements in this area — the ID's prefix, as `LIFE` in
    /// `LIFE-001`; repeatable.
    #[arg(long, value_name = "AREA")]
    area: Vec<String>,
    /// Only requirements at this RFC 2119 level; repeatable.
    #[arg(long, value_enum)]
    level: Vec<LevelArg>,
    /// Only requirements a trace check judges.
    #[arg(long, conflicts_with = "excluded")]
    checked: bool,
    /// Only requirements the registry excludes from trace judgment, with why.
    #[arg(long)]
    excluded: bool,
}

/// `requirements` output formats: a registry has no `JUnit` or SARIF form, so
/// `--help` does not offer one.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    /// Terminal-oriented text.
    Human,
    /// Pretty-printed JSON, in the registry document's shape.
    Json,
}

/// An RFC 2119 level, as `--level` spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LevelArg {
    Must,
    MustNot,
    Should,
    ShouldNot,
    May,
}

impl LevelArg {
    const fn level(self) -> Level {
        match self {
            Self::Must => Level::Must,
            Self::MustNot => Level::MustNot,
            Self::Should => Level::Should,
            Self::ShouldNot => Level::ShouldNot,
            Self::May => Level::May,
        }
    }
}

/// The registry document's shape, holding only the requirements selected —
/// all of them without a filter, so the unfiltered JSON is the registry's own.
#[derive(Serialize)]
struct Selected<'a> {
    revision: ProtocolRevision,
    requirements: Vec<&'a Requirement>,
}

pub(crate) fn run(args: &Args) -> u8 {
    let registry = match requirements_registry(args.registry.as_deref(), args.revision.as_deref()) {
        Ok(registry) => registry,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let selected = match select(&registry, args) {
        Ok(selected) => selected,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let written = match args.format {
        Format::Json => match serde_json::to_string_pretty(&selected) {
            Ok(json) => emit(&format!("{json}\n")),
            Err(error) => {
                eprintln!("error: cannot serialize registry: {error}");
                return EXIT_USAGE;
            }
        },
        Format::Human if args.ids.is_empty() => emit(&list(&selected, args.excluded)),
        Format::Human => emit(&details(&selected)),
    };
    if written { EXIT_OK } else { EXIT_USAGE }
}

/// The requirements the arguments select, in the order named (IDs) or the
/// registry's order (filters alone). An ID or area the registry does not have
/// is an error rather than an empty answer: it is almost always a typo.
fn select<'a>(registry: &'a Registry, args: &Args) -> Result<Selected<'a>, String> {
    let revision = registry.revision();
    let named: Vec<&Requirement> = if args.ids.is_empty() {
        registry.requirements().iter().collect()
    } else {
        let mut named = Vec::new();
        for id in &args.ids {
            let requirement = registry.get(&id.to_ascii_uppercase()).ok_or_else(|| {
                let mut message = format!("revision {revision} has no requirement {id}");
                if args.registry.is_none()
                    && let Some(other) = builtin_revision_with(&id.to_ascii_uppercase())
                {
                    // The usual case: an ID read off a report of the other
                    // revision, looked up at the default (newest) one.
                    let _ = write!(
                        message,
                        "; it is a {other} requirement: pass --revision {other}"
                    );
                }
                message
            })?;
            named.push(requirement);
        }
        named
    };
    let areas: Vec<String> = args
        .area
        .iter()
        .map(|area| area.to_ascii_uppercase())
        .collect();
    if let Some(unknown) = areas.iter().find(|area| {
        !registry
            .requirements()
            .iter()
            .any(|requirement| requirement.id.area() == area.as_str())
    }) {
        return Err(format!("revision {revision} has no area {unknown}"));
    }
    let requirements = named
        .into_iter()
        .filter(|requirement| {
            (areas.is_empty() || areas.iter().any(|area| requirement.id.area() == area))
                && (args.level.is_empty()
                    || args
                        .level
                        .iter()
                        .any(|level| level.level() == requirement.level))
                && (!args.checked
                    || matches!(requirement.verification, Verification::Checks { .. }))
                && (!args.excluded
                    || matches!(requirement.verification, Verification::Excluded { .. }))
        })
        .collect();
    Ok(Selected {
        revision,
        requirements,
    })
}

/// One line per requirement, as `requirements` has always printed them —
/// and, with `reasons` (`--excluded`, which promises "with why"), each
/// exclusion's reason on the line after it.
fn list(selected: &Selected<'_>, reasons: bool) -> String {
    let mut out = format!("requirement registry — revision {}\n", selected.revision);
    for requirement in &selected.requirements {
        let verification = match &requirement.verification {
            Verification::Checks { checks } => format!("checks: {}", checks.join(", ")),
            Verification::Excluded { .. } => "excluded".to_owned(),
            // Foreign #[non_exhaustive] enum: future arms surface visibly.
            _ => "unrecognized verification".to_owned(),
        };
        let _ = writeln!(
            out,
            "  {} {:<9} ({}) — {}",
            requirement.id,
            requirement.level.keyword(),
            verification,
            requirement.source.quote
        );
        if reasons && let Verification::Excluded { exclusion } = &requirement.verification {
            let _ = writeln!(out, "      why: {exclusion}");
        }
    }
    out
}

/// Each requirement in full: what binds whom, the clause and where it is
/// published, and how a trace is judged against it — or why it is not.
fn details(selected: &Selected<'_>) -> String {
    let mut out = String::new();
    for (index, requirement) in selected.requirements.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let actor = match requirement.actor {
            Actor::Server => "the server",
            Actor::Client => "the client",
            Actor::Both => "client and server",
            // Foreign #[non_exhaustive] enum: future arms surface visibly.
            _ => "an unrecognized party",
        };
        let _ = writeln!(
            out,
            "{} — {}, binding {actor} — revision {}",
            requirement.id,
            requirement.level.keyword(),
            selected.revision
        );
        let _ = writeln!(out, "  spec: \"{}\"", requirement.source.quote);
        let _ = writeln!(out, "  see:  {}", requirement.source.url(selected.revision));
        if let Some(capability) = &requirement.capability {
            let _ = writeln!(
                out,
                "  capability: {capability} — judged only in a session that declares it"
            );
        }
        let _ = match &requirement.verification {
            Verification::Checks { checks } => writeln!(out, "  checks: {}", checks.join(", ")),
            Verification::Excluded { exclusion } => writeln!(out, "  excluded: {exclusion}"),
            _ => writeln!(out, "  unrecognized verification"),
        };
    }
    out
}

/// The registry `requirements` prints: a custom file, or a built-in revision (the
/// newest by default).
fn requirements_registry(
    registry_path: Option<&std::path::Path>,
    revision: Option<&str>,
) -> Result<Registry, String> {
    if let Some(path) = registry_path {
        return load_registry(path);
    }
    let set = load_registry_set(None)?;
    let revision = match revision {
        Some(text) => text
            .parse::<ProtocolRevision>()
            .map_err(|error| error.to_string())?,
        None => set
            .latest()
            .ok_or_else(|| "the built-in registry set describes no revision".to_owned())?,
    };
    set.registry(revision).ok_or_else(|| {
        format!(
            "no built-in registry for revision {revision} (supported: {})",
            set.revisions()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// The built-in revision whose registry has `id`, when one does (newest first).
fn builtin_revision_with(id: &str) -> Option<ProtocolRevision> {
    let set = load_registry_set(None).ok()?;
    let mut revisions = set.revisions().to_vec();
    revisions.sort_unstable_by(|a, b| b.cmp(a));
    revisions.into_iter().find(|revision| {
        set.registry(*revision)
            .is_some_and(|registry| registry.get(id).is_some())
    })
}
