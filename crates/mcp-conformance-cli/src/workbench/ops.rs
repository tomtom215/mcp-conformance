// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The library operations the workbench's API and `mcp-conformance library`
//! share, so a change made from the browser and one made from the command
//! line are the same change.

use mcp_conformance_cli::import::{self, Format, Options};
use mcp_conformance_core::trace::TransportKind;
use serde_json::Value;

use super::clock;
use super::meta::Meta;
use super::redact::{self, Rules};
use super::state::{Failure, Workbench};
use super::tar::Archive;

pub(crate) fn transport(name: Option<&str>) -> Result<Option<TransportKind>, Failure> {
    match name {
        None | Some("") => Ok(None),
        Some("stdio") => Ok(Some(TransportKind::Stdio)),
        Some("streamable-http") => Ok(Some(TransportKind::StreamableHttp)),
        Some(other) => Err(Failure::Invalid(format!(
            "transport {other:?}: say stdio or streamable-http"
        ))),
    }
}

/// Imports a recording as a new entry (the file name a person chose, so
/// [`crate::workbench::library::id_from_name`] makes it an id).
pub(crate) fn import_document(
    wb: &mut Workbench,
    name: &str,
    text: &str,
    format: Option<&str>,
    transport_name: Option<&str>,
) -> Result<Value, Failure> {
    let format = match format {
        None | Some("" | "auto") => import::detect(text).ok_or_else(|| {
            Failure::Invalid(
                "the file is neither a trace, an mcpsnoop capture nor an Inspector export"
                    .to_owned(),
            )
        })?,
        Some(name) => Format::named(name).ok_or_else(|| {
            Failure::Invalid(format!("format {name:?}: say trace, mcpsnoop or inspector"))
        })?,
    };
    let mut options = Options::default();
    if let Some(kind) = transport(transport_name)? {
        options = options.transport(kind);
    }
    let imported =
        import::import(text, format, &options).map_err(|error| Failure::Invalid(error.0))?;
    let mut meta = Meta::new(if format == Format::Trace {
        "recorded"
    } else {
        "imported"
    });
    meta.source.importer = Some(format.name().to_owned());
    meta.source.imported_from = Some(name.rsplit(['/', '\\']).next().unwrap_or(name).to_owned());
    meta.source.not_recorded = imported.not_recorded;
    meta.added_at = Some(clock::now());
    meta.recorded_at = imported.recorded_at;
    let id = wb.add(name, imported.trace.as_bytes(), &meta)?;
    Ok(wb.entry(&id)?.summary())
}

/// Adds a redacted copy of entry `id` as a new entry.
pub(crate) fn redact_document(
    wb: &mut Workbench,
    id: &str,
    rules: &Rules,
) -> Result<Value, Failure> {
    let text = wb.text(id)?;
    let redacted = redact::redact(&text, rules).map_err(|error| Failure::Invalid(error.0))?;
    let original = wb.entry(id)?;
    let mut meta = Meta::new("redacted");
    meta.tags.clone_from(&original.meta.tags);
    meta.source
        .importer
        .clone_from(&original.meta.source.importer);
    meta.source
        .not_recorded
        .clone_from(&original.meta.source.not_recorded);
    meta.source.redacted_from = Some(id.to_owned());
    meta.source.redactions = redacted.summary;
    meta.added_at = Some(clock::now());
    meta.recorded_at = original.when();
    let copy = wb.add(&format!("{id}.redacted"), redacted.text.as_bytes(), &meta)?;
    Ok(wb.entry(&copy)?.summary())
}

/// The entries recorded more than `days` days before `now`; deleted when
/// `apply`. An entry with no date is never selected.
pub(crate) fn retention_select(wb: &Workbench, days: u32, now: &str) -> Vec<String> {
    let today = clock::day(now).unwrap_or_default();
    wb.entries
        .values()
        .filter(|entry| {
            entry
                .day()
                .and_then(|day| clock::age_in_days(&day, today))
                .is_some_and(|age| age > i64::from(days))
        })
        .map(|entry| entry.id.clone())
        .collect()
}

/// The most entries one export holds.
pub(crate) const EXPORT_MAX_ENTRIES: usize = 100;

/// The most trace bytes one export holds, before the archive's framing and
/// the sidecars: the import body's limit, by the same reasoning — a
/// recording at the toolkit's message limit, with room — since the archive
/// is built in memory before it is sent.
pub(crate) const EXPORT_MAX_BYTES: u64 = 192 * 1024 * 1024;

/// An export: the archive, and how many entries it holds.
#[derive(Debug)]
pub(crate) struct Export {
    pub(crate) bytes: Vec<u8>,
    pub(crate) entries: usize,
}

/// `ids` each once, in the order first named.
pub(crate) fn export_unique<'a>(ids: &[&'a str]) -> Vec<&'a str> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// A tar archive of the entries `ids` — each once, however often it is
/// named — as `<id>.jsonl` and its sidecar `<id>.meta.json`, stamped with
/// the trace's modification time. Refused before anything is read when more
/// than [`EXPORT_MAX_ENTRIES`] entries are named or their traces hold more
/// than [`EXPORT_MAX_BYTES`].
pub(crate) fn export_archive(wb: &Workbench, ids: &[&str]) -> Result<Export, Failure> {
    export_within(wb, ids, EXPORT_MAX_ENTRIES, EXPORT_MAX_BYTES)
}

fn export_within(
    wb: &Workbench,
    ids: &[&str],
    max_entries: usize,
    max_bytes: u64,
) -> Result<Export, Failure> {
    let ids = export_unique(ids);
    if ids.is_empty() {
        return Err(Failure::Invalid("name the entries to export".to_owned()));
    }
    if ids.len() > max_entries {
        return Err(Failure::Invalid(format!(
            "an export holds at most {max_entries} entries; {} were named",
            ids.len()
        )));
    }
    let mut total = 0_u64;
    for id in &ids {
        total = total.saturating_add(wb.entry(id)?.size);
    }
    if total > max_bytes {
        return Err(Failure::Invalid(format!(
            "an export holds at most {max_bytes} bytes of traces; the entries named hold {total}"
        )));
    }
    let mut archive = Archive::default();
    for id in &ids {
        let entry = wb.entry(id)?;
        let mtime = entry
            .modified
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs());
        let trace = std::fs::read(wb.library.trace_path(id))?;
        archive
            .add(&format!("{id}.jsonl"), &trace, mtime)
            .map_err(Failure::Invalid)?;
        archive
            .add(
                &format!("{id}.meta.json"),
                entry.meta.to_text().as_bytes(),
                mtime,
            )
            .map_err(Failure::Invalid)?;
    }
    Ok(Export {
        bytes: archive.finish(),
        entries: ids.len(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::workbench::fixture;

    #[test]
    fn a_trace_is_recorded_and_another_format_imported() {
        let (mut wb, dir) = fixture::workbench("import-kind");
        let trace = std::fs::read_to_string(dir.join("everything-stdio.jsonl")).unwrap();
        let entry = import_document(&mut wb, "a/b\\copy.jsonl", &trace, None, None).unwrap();
        assert_eq!(entry["source"]["kind"], "recorded");
        assert_eq!(entry["source"]["importer"], "trace");
        assert_eq!(wb.entry("copy").unwrap().meta.source.kind, "recorded");
        let capture = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../conformance/mcpsnoop/native/stdio-2025-11-25.mcpsnoop.jsonl"),
        )
        .unwrap();
        let entry = import_document(&mut wb, "snoop.jsonl", &capture, Some("auto"), None).unwrap();
        assert_eq!(entry["source"]["kind"], "imported");
    }

    /// The byte bound is judged from the entries' sizes before any trace is
    /// read, and an export at it goes through.
    #[test]
    fn an_export_is_refused_before_reading_when_its_traces_exceed_the_bound() {
        let (wb, _dir) = fixture::workbench("export-bytes");
        let ids = ["everything-http", "tool-005-name-length"];
        let sizes: u64 = ids.iter().map(|id| wb.entry(id).unwrap().size).sum();
        let refused = export_within(&wb, &ids, EXPORT_MAX_ENTRIES, sizes - 1).unwrap_err();
        assert_eq!(
            refused,
            Failure::Invalid(format!(
                "an export holds at most {} bytes of traces; the entries named hold {sizes}",
                sizes - 1
            ))
        );
        let export = export_within(&wb, &ids, EXPORT_MAX_ENTRIES, sizes).unwrap();
        assert_eq!(export.entries, 2);
        assert_eq!(crate::workbench::tar::tests::read(&export.bytes).len(), 4);
        // The bounds themselves: named by the README, held by the API.
        assert_eq!(EXPORT_MAX_ENTRIES, 100);
        assert_eq!(EXPORT_MAX_BYTES, 192 * 1024 * 1024);
        assert!(matches!(
            export_archive(&wb, &[]),
            Err(Failure::Invalid(why)) if why == "name the entries to export"
        ));
    }

    #[test]
    fn retention_keeps_what_is_exactly_as_old_as_the_limit() {
        let (wb, _dir) = fixture::workbench("retention-edge");
        let now = "2026-10-12T23:59:59Z";
        let today: Vec<&str> = wb
            .entries
            .values()
            .filter(|entry| entry.day().as_deref() == Some("2026-10-12"))
            .map(|entry| entry.id.as_str())
            .collect();
        assert!(
            !today.is_empty(),
            "the fixtures hold a recording of that day"
        );
        let selected = retention_select(&wb, 0, now);
        for id in &today {
            assert!(!selected.iter().any(|s| s == id), "{id} is 0 days old");
        }
        assert_eq!(selected.len(), wb.entries.len() - today.len());
    }
}
