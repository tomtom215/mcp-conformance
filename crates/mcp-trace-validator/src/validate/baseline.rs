// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `validate`'s baseline flags (ADR-0025): reading the file, writing one, the
//! clock, and what a run says about its baseline in each format.
//!
//! The library marks findings and reports the entries that did not do their
//! job; this module decides, at the command-line edge, what that costs — a
//! stale entry fails the run unless `--allow-stale-baseline` — and is the one
//! place the validator reads the clock, and only for an entry with an `expires`.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use mcp_trace_validator::baseline::{
    Baseline, BaselineError, Date, Entry, Notes, TraceReports, record,
};
use mcp_trace_validator::report::{BaselineTally, Party, Report, printable};
use serde_json::{Value, json};

use super::Judged;
use crate::Format;

/// The baseline flags as given.
pub(crate) struct Settings<'a> {
    /// `--baseline`: the file to read.
    pub(crate) read: Option<&'a Path>,
    /// `--write-baseline`: the file to write.
    pub(crate) write: Option<&'a Path>,
    /// `--allow-stale-baseline`.
    pub(crate) allow_stale: bool,
}

/// What a run's baseline came to: nothing, when none was in use.
#[derive(Default)]
pub(crate) struct Run {
    /// The entries that did not do their job.
    pub(crate) notes: Notes,
    /// How the run's findings fell, when a baseline was applied.
    pub(crate) tally: Option<BaselineTally>,
}

impl Run {
    /// Whether the baseline alone fails the run: a stale entry not allowed.
    pub(crate) const fn fails(&self) -> bool {
        self.notes.fails()
    }
}

/// Refuses what cannot work before any trace is read: the target of
/// `--write-baseline`, which is never overwritten unless it is the baseline in
/// use (whose reasons it keeps), and never stdout, which carries the report.
pub(crate) fn check(settings: &Settings<'_>, format: Format, filtered: bool) -> Result<(), String> {
    if filtered && !matches!(format, Format::Human) {
        return Err(
            "--area and --level choose what the human report lists; the other formats always \
             carry every clause and every finding, so the filter would be ignored. Drop it, or \
             use --format human"
                .to_owned(),
        );
    }
    let Some(target) = settings.write else {
        return Ok(());
    };
    if target.as_os_str() == "-" {
        return Err(
            "--write-baseline cannot write to stdout, which carries the report; name a file"
                .to_owned(),
        );
    }
    if target.exists() && !settings.read.is_some_and(|read| same_file(read, target)) {
        return Err(format!(
            "{} exists, and --write-baseline will not overwrite a file it has not read: it \
             would lose the reasons in it. Pass it as --baseline too, to refresh it and keep \
             them, or remove it",
            target.display()
        ));
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Reads `--baseline`.
pub(crate) fn load(settings: &Settings<'_>) -> Result<Option<Baseline>, String> {
    let Some(path) = settings.read else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Baseline::parse(&text)
        .map(Some)
        .map_err(|error: BaselineError| format!("{}: {error}", path.display()))
}

/// Today's date, UTC, from the system clock.
fn today() -> Result<Date, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs() / 86_400).ok())
        .and_then(Date::from_unix_days)
        .ok_or_else(|| {
            "cannot read today's date from the system clock, which an `expires` in the baseline \
             needs"
                .to_owned()
        })
}

/// The traces as the library names them.
fn named(judged: &[Judged]) -> Vec<(Option<&str>, &[Report])> {
    judged
        .iter()
        .map(|one| {
            (
                (one.trace != "-").then_some(one.trace.as_str()),
                one.reports.as_slice(),
            )
        })
        .collect()
}

/// Writes `--write-baseline` and applies the baseline in force to `judged`.
///
/// With `--write-baseline` the baseline in force is the one just written, so the
/// run that writes it prints the report the next run will; without a trace
/// that could not be judged (`unjudged`), because a baseline written from the
/// traces that could would silently lack the findings of the one that could not.
pub(crate) fn finish(
    judged: &mut [Judged],
    unjudged: bool,
    settings: &Settings<'_>,
    previous: Option<Baseline>,
    view: Option<Party>,
) -> Result<Run, String> {
    let baseline = match settings.write {
        Some(path) if !unjudged => {
            let recorded = record(&named(judged), view, previous.as_ref());
            std::fs::write(path, recorded.baseline.to_json())
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            eprintln!("{}", wrote(path, &recorded, previous.is_some()));
            Some(recorded.baseline)
        }
        Some(path) => {
            eprintln!(
                "note: --write-baseline wrote nothing to {}: a trace could not be judged, and \
                 a baseline written without its findings would be incomplete",
                path.display()
            );
            previous
        }
        None => previous,
    };
    let Some(baseline) = baseline else {
        return Ok(Run::default());
    };
    let today = if baseline.has_expiry() {
        Some(today()?)
    } else {
        None
    };
    let mut traces: Vec<TraceReports<'_>> = judged
        .iter_mut()
        .map(|one| {
            let name = (one.trace != "-").then_some(one.trace.as_str());
            TraceReports::new(name, one.reports.as_mut_slice())
        })
        .collect();
    let applied = baseline
        .apply(&mut traces, today)
        .map_err(|error| error.to_string())?;
    drop(traces);
    for one in judged.iter_mut() {
        if let Some(multi) = &mut one.multi {
            multi.adopt_baseline(&one.reports);
        }
    }
    Ok(Run {
        notes: applied.notes.allowing_stale(settings.allow_stale),
        tally: Some(applied.tally),
    })
}

/// The line on stderr after a baseline is written.
fn wrote(
    path: &Path,
    recorded: &mcp_trace_validator::baseline::Recorded,
    refreshed: bool,
) -> String {
    let count = recorded.baseline.entries().len();
    let mut line = format!(
        "wrote {count} {} to {}",
        if count == 1 { "entry" } else { "entries" },
        path.display()
    );
    if refreshed {
        let _ = write!(
            line,
            " ({} kept with their reasons and expiry, {} added with the generated reason, {} \
             dropped for matching no finding)",
            recorded.carried, recorded.added, recorded.dropped
        );
    } else if count > 0 {
        let _ = write!(
            line,
            "; each carries the generated reason, which every later run counts as unreviewed — \
             replace it with why the finding is accepted"
        );
    }
    line
}

fn entry_json(entry: &Entry) -> Value {
    let mut object = json!({ "fingerprint": entry.fingerprint, "reason": entry.reason });
    if let Some(day) = entry.expires {
        object["expires"] = json!(day.to_string());
    }
    object
}

impl Run {
    /// The stderr line saying why this run exits `1`, when the baseline is why.
    pub(crate) fn note_failure(&self) {
        if self.fails() {
            let count = self.notes.stale.len();
            eprintln!(
                "note: --baseline — {count} {} no finding of this run, so this run exits 1 \
                 (pass --allow-stale-baseline to list {} without failing)",
                if count == 1 {
                    "entry matches"
                } else {
                    "entries match"
                },
                if count == 1 { "it" } else { "them" }
            );
        }
    }

    /// The notes for the end of a human report; empty when there are none.
    pub(crate) fn human(&self) -> String {
        let mut text = String::new();
        let notes = &self.notes;
        if !notes.stale.is_empty() {
            let count = notes.stale.len();
            let _ = writeln!(
                text,
                "baseline: {count} {} no finding of this run, so what {} accepted no longer \
                 occurs{}:",
                if count == 1 {
                    "entry matches"
                } else {
                    "entries match"
                },
                if count == 1 { "it" } else { "they" },
                if notes.allow_stale {
                    " (stale entries are allowed: this does not fail the run)".to_owned()
                } else {
                    format!(
                        "; this run exits 1 until {} removed, or pass --allow-stale-baseline",
                        if count == 1 { "it is" } else { "they are" }
                    )
                }
            );
            entries(&mut text, &notes.stale, false);
        }
        if !notes.lapsed.is_empty() {
            let count = notes.lapsed.len();
            let _ = writeln!(
                text,
                "baseline: {count} {} expired and accept{} nothing; {} counts again:",
                if count == 1 {
                    "entry has"
                } else {
                    "entries have"
                },
                if count == 1 { "s" } else { "" },
                if count == 1 {
                    "the finding it matched"
                } else {
                    "each finding they matched"
                }
            );
            entries(&mut text, &notes.lapsed, true);
        }
        text
    }

    /// Adds what the document that stands for the run can carry — the stale and
    /// lapsed entries — to the `baseline` member its report already has.
    pub(crate) fn annotate(&self, document: &mut Value) {
        let Some(object) = document.get_mut("baseline").and_then(Value::as_object_mut) else {
            return;
        };
        self.add_lists(object);
    }

    /// The envelope's `baseline` member for several traces: the findings accepted
    /// and not across all of them, and the run's stale and lapsed entries.
    pub(crate) fn envelope(&self) -> Option<Value> {
        let tally = self.tally?;
        let Value::Object(mut object) = json!(tally) else {
            return None;
        };
        self.add_lists(&mut object);
        Some(Value::Object(object))
    }

    fn add_lists(&self, object: &mut serde_json::Map<String, Value>) {
        if !self.notes.stale.is_empty() {
            object.insert(
                "stale".to_owned(),
                self.notes.stale.iter().map(entry_json).collect(),
            );
        }
        if !self.notes.lapsed.is_empty() {
            object.insert(
                "lapsed".to_owned(),
                self.notes.lapsed.iter().map(entry_json).collect(),
            );
        }
    }
}

fn entries(text: &mut String, list: &[Entry], with_day: bool) {
    for entry in list {
        let _ = writeln!(text, "  {}", printable(&entry.fingerprint));
        let day = entry
            .expires
            .filter(|_| with_day)
            .map_or_else(String::new, |day| format!(" (expired {day})"));
        let _ = writeln!(text, "    reason: {}{day}", printable(&entry.reason));
    }
}
