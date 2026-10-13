// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-conformance library`: the workbench's library from the command line.
//!
//! Each subcommand is the operation the workbench's page performs
//! ([`crate::workbench::ops`], [`crate::workbench::stats`]), so a number on a
//! dashboard is reproduced by `library stats --format json`, and an import or
//! a redaction made here is the one the page would make. Exit `0` when it
//! succeeded, `2` when it could not be done.

use core::fmt::Write as _;
use std::io::Write as _;

use crate::args::{LibraryArgs, LibraryCommand, StatsFormat};
use crate::workbench::clock;
use crate::workbench::ops;
use crate::workbench::redact::{DEFAULT_HEADERS, Rules};
use crate::workbench::state::{Failure, Workbench};
use crate::workbench::stats;
use crate::{EXIT_OK, EXIT_USAGE};

/// Runs a `library` subcommand; returns the exit code.
pub(crate) fn run(args: LibraryArgs) -> u8 {
    let mut workbench = match Workbench::open(&args.dir.library) {
        Ok(workbench) => workbench,
        Err(error) => {
            eprintln!(
                "mcp-conformance library: cannot open {}: {error}",
                args.dir.library.display()
            );
            return EXIT_USAGE;
        }
    };
    match execute(&mut workbench, args.command) {
        Ok(output) => {
            let mut stdout = std::io::stdout().lock();
            if stdout.write_all(output.as_bytes()).is_err() {
                return EXIT_USAGE;
            }
            EXIT_OK
        }
        Err(failure) => {
            let why = match failure {
                Failure::NotFound(why) | Failure::Invalid(why) | Failure::Io(why) => why,
            };
            eprintln!("mcp-conformance library: {why}");
            EXIT_USAGE
        }
    }
}

/// One subcommand's output.
// One arm per subcommand, each a few lines: splitting the match would only
// move the dispatch elsewhere.
#[allow(clippy::too_many_lines)]
pub(crate) fn execute(wb: &mut Workbench, command: LibraryCommand) -> Result<String, Failure> {
    let id_of = |value: &serde_json::Value| value["id"].as_str().unwrap_or_default().to_owned();
    Ok(match command {
        LibraryCommand::Stats { format } => {
            let numbers = stats::compute(wb.entries.values());
            match format {
                StatsFormat::Json => stats::to_json(&numbers),
                StatsFormat::Human => stats::to_human(&numbers),
            }
        }
        LibraryCommand::List => wb.entries.values().fold(String::new(), |mut out, entry| {
            let _ = writeln!(
                out,
                "{}\t{}\t{}\t{}",
                entry.id,
                entry.verdict(),
                entry.day().unwrap_or_else(|| "undated".to_owned()),
                stats::visible(entry.facts.server.as_deref().unwrap_or("-"))
            );
            out
        }),
        LibraryCommand::Import {
            file,
            from,
            transport,
        } => {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| Failure::Io(format!("{}: {error}", file.display())))?;
            let name = file
                .file_name()
                .map_or_else(|| "session".into(), |n| n.to_string_lossy().into_owned());
            let entry = ops::import_document(wb, &name, &text, Some(&from), transport.as_deref())?;
            format!("imported {}\n", id_of(&entry))
        }
        LibraryCommand::Redact {
            id,
            headers,
            pointers,
            meta,
            strings,
        } => {
            let headers = if headers.is_empty() {
                DEFAULT_HEADERS.iter().map(|h| (*h).to_owned()).collect()
            } else {
                headers
            };
            let rules = Rules {
                headers,
                pointers,
                meta,
                strings,
            };
            let copy = ops::redact_document(wb, &id, &rules)?;
            format!("redacted copy {}\n", id_of(&copy))
        }
        LibraryCommand::Prune { retain_days, apply } => {
            let selected = ops::retention_select(wb, retain_days, &clock::now());
            let mut out = String::new();
            for id in &selected {
                if apply {
                    wb.delete(id)?;
                }
                let _ = writeln!(
                    out,
                    "{} {id}",
                    if apply { "deleted" } else { "would delete" }
                );
            }
            if selected.is_empty() {
                let _ = writeln!(out, "nothing recorded more than {retain_days} days ago");
            }
            out
        }
        LibraryCommand::Tag { id, tags } => {
            wb.set_tags(&id, &tags)?;
            format!("{id}: {}\n", wb.entry(&id)?.meta.tags.join(", "))
        }
        LibraryCommand::Delete { ids } => {
            for id in &ids {
                wb.entry(id)?;
            }
            let mut out = String::new();
            for id in ids {
                wb.delete(&id)?;
                let _ = writeln!(out, "deleted {id}");
            }
            out
        }
        LibraryCommand::Export { ids, output } => {
            let names: Vec<&str> = ids.iter().map(String::as_str).collect();
            let bytes = ops::export_archive(wb, &names)?;
            std::fs::write(&output, bytes)
                .map_err(|error| Failure::Io(format!("{}: {error}", output.display())))?;
            format!(
                "exported {} recordings to {}\n",
                ids.len(),
                output.display()
            )
        }
    })
}
