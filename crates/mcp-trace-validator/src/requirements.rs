// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `requirements` subcommand: print a registry.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fmt::Write as _;

use mcp_conformance_core::requirement::{Registry, Verification};
use mcp_conformance_core::revision::ProtocolRevision;

use crate::emit::emit;
use crate::{EXIT_OK, EXIT_USAGE, Format, load_registry, load_registry_set};

pub(crate) fn run(
    format: Format,
    registry_path: Option<&std::path::Path>,
    revision: Option<&str>,
) -> u8 {
    let registry = match requirements_registry(registry_path, revision) {
        Ok(registry) => registry,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let written = match format {
        Format::Junit | Format::Sarif => {
            eprintln!(
                "error: --format junit and --format sarif apply to validate, not requirements"
            );
            return EXIT_USAGE;
        }
        Format::Json => match serde_json::to_string_pretty(&registry) {
            Ok(json) => emit(&format!("{json}\n")),
            Err(error) => {
                eprintln!("error: cannot serialize registry: {error}");
                return EXIT_USAGE;
            }
        },
        Format::Human => {
            let mut out = format!("requirement registry — revision {}\n", registry.revision());
            for requirement in registry.requirements() {
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
            }
            emit(&out)
        }
    };
    if written { EXIT_OK } else { EXIT_USAGE }
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
