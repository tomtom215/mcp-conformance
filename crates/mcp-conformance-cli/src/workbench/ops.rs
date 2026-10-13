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

/// A tar archive of the entries `ids`: each trace as `<id>.jsonl` and its
/// sidecar as `<id>.meta.json`, stamped with the trace's modification time.
pub(crate) fn export_archive(wb: &Workbench, ids: &[&str]) -> Result<Vec<u8>, Failure> {
    if ids.is_empty() {
        return Err(Failure::Invalid("name the entries to export".to_owned()));
    }
    let mut archive = Archive::default();
    for id in ids {
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
    Ok(archive.finish())
}
