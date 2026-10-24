// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The library as the workbench holds it: every entry loaded, kept in step
//! with the directory.
//!
//! [`Workbench::refresh`] rescans the directory and reloads an entry only
//! when its trace's size or modification time moved, so a recorder writing
//! into the library (a running `watch`, a capture) shows up on the next
//! request, and an unchanged library costs a directory listing.
//!
//! The parsed events of the [`EVENTS_CACHED`] traces most recently viewed
//! are kept, keyed by the SHA-256 of the bytes they were parsed from — the
//! same key the sidecar's verdict cache uses — so following a session (the
//! page polls it), comparing two, or downloading a report does not parse
//! the whole trace again, and a trace that changed (another hash) is never
//! served stale. A large line is parsed into a tree many times its size
//! (the crate README gives the measurement), which is why the cache is
//! small.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::reader::{self, Limits};

use super::entry::{self, Entry};
use super::library::{Library, Skipped, valid_id};
use super::meta::Meta;

/// How many traces' parsed events are kept, most recently viewed first.
pub(crate) const EVENTS_CACHED: usize = 4;

/// The library and its entries.
#[derive(Debug)]
pub(crate) struct Workbench {
    pub(crate) library: Library,
    pub(crate) entries: BTreeMap<String, Entry>,
    pub(crate) skipped: Vec<Skipped>,
    /// Parsed events by the SHA-256 of their bytes, most recently used first.
    events: VecDeque<(String, Arc<Vec<TraceEvent>>)>,
}

/// A request about an entry that cannot be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// No entry has this id.
    NotFound(String),
    /// The request is malformed.
    Invalid(String),
    /// The file system refused.
    Io(String),
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl Workbench {
    /// The library at `root`, loaded.
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        let mut workbench = Self {
            library: Library::open(root)?,
            entries: BTreeMap::new(),
            skipped: Vec::new(),
            events: VecDeque::new(),
        };
        workbench.refresh()?;
        Ok(workbench)
    }

    /// Brings the entries in step with the directory.
    pub(crate) fn refresh(&mut self) -> io::Result<()> {
        let (ids, skipped) = self.library.scan()?;
        self.skipped = skipped;
        self.entries.retain(|id, _| ids.binary_search(id).is_ok());
        for id in ids {
            let path = self.library.trace_path(&id);
            let Ok(stat) = fs::metadata(&path) else {
                self.entries.remove(&id);
                continue;
            };
            let modified = stat.modified().ok();
            let unchanged = self
                .entries
                .get(&id)
                .is_some_and(|entry| entry.size == stat.len() && entry.modified == modified);
            if !unchanged {
                self.load(&id, modified)?;
            }
        }
        Ok(())
    }

    /// (Re)loads one entry from disk, writing a recomputed verdict cache
    /// back to its sidecar unless the sidecar was unreadable.
    fn load(&mut self, id: &str, modified: Option<std::time::SystemTime>) -> io::Result<()> {
        let bytes = fs::read(self.library.trace_path(id))?;
        let read = self.library.read_meta(id);
        let problem = read.problem.clone();
        let (entry, stale) = Entry::load(id, &bytes, read.meta, read.problem, modified);
        if stale && problem.is_none() {
            // A read-only library still works; it only judges again next time.
            let _ = self.library.write_meta(id, &entry.meta);
        }
        self.entries.insert(id.to_owned(), entry);
        Ok(())
    }

    /// Entry `id`.
    pub(crate) fn entry(&self, id: &str) -> Result<&Entry, Failure> {
        self.entries
            .get(id)
            .ok_or_else(|| Failure::NotFound(format!("no entry {id}")))
    }

    /// Entry `id`'s trace text.
    pub(crate) fn text(&self, id: &str) -> Result<String, Failure> {
        self.entry(id)?;
        let bytes = fs::read(self.library.trace_path(id))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Entry `id`'s events, as far as they parse: from the cache when it
    /// holds the events of the bytes the entry was loaded from, else parsed
    /// from the file and cached under the hash of the bytes actually read.
    pub(crate) fn events(&mut self, id: &str) -> Result<Arc<Vec<TraceEvent>>, Failure> {
        let sha256 = self.entry(id)?.sha256.clone();
        if let Some(at) = self.events.iter().position(|(key, _)| *key == sha256)
            && let Some(hit) = self.events.remove(at)
        {
            let events = Arc::clone(&hit.1);
            self.events.push_front(hit);
            return Ok(events);
        }
        let bytes = fs::read(self.library.trace_path(id))?;
        let text = String::from_utf8_lossy(&bytes);
        let events = Arc::new(
            reader::parse_trace(entry::complete(&text), &Limits::default()).unwrap_or_default(),
        );
        self.events
            .push_front((entry::sha256(&bytes), Arc::clone(&events)));
        self.events.truncate(EVENTS_CACHED);
        Ok(events)
    }

    /// Adds a recording; returns its id.
    pub(crate) fn add(&mut self, name: &str, trace: &[u8], meta: &Meta) -> Result<String, Failure> {
        let id = self.library.add(name, trace, meta)?;
        let modified = fs::metadata(self.library.trace_path(&id))
            .ok()
            .and_then(|stat| stat.modified().ok());
        self.load(&id, modified)?;
        Ok(id)
    }

    /// Replaces entry `id`'s tags.
    pub(crate) fn set_tags(&mut self, id: &str, tags: &[String]) -> Result<(), Failure> {
        let events = self.events(id)?;
        let library = self.library.clone();
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| Failure::NotFound(format!("no entry {id}")))?;
        if let Some(problem) = &entry.meta_problem {
            return Err(Failure::Invalid(format!(
                "{id}: {problem}; fix or remove the sidecar before tagging"
            )));
        }
        if let Some(bad) = tags
            .iter()
            .find(|tag| tag.trim().chars().any(char::is_control))
        {
            return Err(Failure::Invalid(format!(
                "a tag cannot hold a control character: {bad:?}"
            )));
        }
        entry.meta.set_tags(tags.iter().map(String::as_str));
        library.write_meta(id, &entry.meta)?;
        entry.reindex(&events);
        Ok(())
    }

    /// Deletes entry `id`.
    pub(crate) fn delete(&mut self, id: &str) -> Result<(), Failure> {
        if !valid_id(id) {
            return Err(Failure::Invalid(format!("not an entry id: {id}")));
        }
        self.entry(id)?;
        self.library.delete(id)?;
        self.entries.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
