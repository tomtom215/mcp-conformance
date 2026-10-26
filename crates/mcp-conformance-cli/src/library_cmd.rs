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
            let mut out = format!("redacted copy {}\n", id_of(&copy));
            for rule in copy["source"]["redactions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
            {
                let _ = writeln!(out, "  {rule}");
            }
            // The defaults touch headers only; a secret inside a message is
            // removed only by a rule that names it.
            let _ = writeln!(
                out,
                "  payload values are kept unless a rule names them — arguments, results, _meta: \
                 --pointer /params/arguments/<name> (a JSON pointer; `*` matches every member), \
                 --meta (every _meta value), --string <text> (a literal, wherever it occurs); \
                 read the copy before sharing it"
            );
            out
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
            let export = ops::export_archive(wb, &names)?;
            std::fs::write(&output, export.bytes)
                .map_err(|error| Failure::Io(format!("{}: {error}", output.display())))?;
            format!(
                "exported {} recordings to {}\n",
                export.entries,
                output.display()
            )
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::args::LibraryDir;

    #[test]
    fn a_library_that_cannot_be_opened_or_an_operation_that_fails_exits_2() {
        let dir = crate::workbench::fixture::library("library-cmd");
        let args = |library: std::path::PathBuf, command| LibraryArgs {
            dir: LibraryDir { library },
            command,
        };
        let stats = || LibraryCommand::Stats {
            format: StatsFormat::Json,
        };
        assert_eq!(run(args(dir.to_path_buf(), stats())), EXIT_OK);
        assert_eq!(
            run(args(dir.join("everything-http.jsonl"), stats())),
            EXIT_USAGE
        );
        let missing = LibraryCommand::Delete {
            ids: vec!["nope".to_owned()],
        };
        assert_eq!(run(args(dir.to_path_buf(), missing)), EXIT_USAGE);
        let import = LibraryCommand::Import {
            file: dir.join("absent.jsonl"),
            from: "auto".to_owned(),
            transport: None,
        };
        assert_eq!(run(args(dir.to_path_buf(), import)), EXIT_USAGE);
    }

    /// A redaction's summary says what each rule replaced and, plainly, what
    /// no default removes: payload values stay unless a rule names them, and
    /// the options that do are named.
    #[test]
    fn a_redaction_summary_names_the_rules_and_what_is_kept() {
        let (mut wb, dir) = crate::workbench::fixture::workbench("library-redact");
        let redact = |headers: Vec<String>, meta: bool| LibraryCommand::Redact {
            id: "leaky-session".to_owned(),
            headers,
            pointers: Vec::new(),
            meta,
            strings: Vec::new(),
        };
        let output = execute(&mut wb, redact(Vec::new(), false)).unwrap();
        assert!(
            output.starts_with("redacted copy leaky-session.redacted\n"),
            "{output}"
        );
        assert!(
            output.contains(
                "  headers authorization, cookie, set-cookie, proxy-authorization, \
                 mcp-session-id: 10 values\n"
            ),
            "{output}"
        );
        assert!(
            output.contains("payload values are kept"),
            "it says plainly what is not removed: {output}"
        );
        for option in ["--pointer", "--meta", "--string"] {
            assert!(output.contains(option), "{option} is named: {output}");
        }
        assert!(
            !output.contains("fixture-"),
            "the summary names no value: {output}"
        );
        let copy = std::fs::read_to_string(dir.join("leaky-session.redacted.jsonl")).unwrap();
        assert!(
            !copy.contains("fixture-session-7"),
            "the session id is a default"
        );
        assert!(copy.contains("fixture-api-key-3"), "a payload value is not");
        // Named headers replace the defaults; the note about payloads stays.
        let output = execute(&mut wb, redact(vec!["X-Key".to_owned()], true)).unwrap();
        assert!(output.contains("  headers X-Key: 0 values\n"), "{output}");
        assert!(output.contains("  _meta members: 2 values\n"), "{output}");
        assert!(output.contains("payload values are kept"), "{output}");
    }
}
