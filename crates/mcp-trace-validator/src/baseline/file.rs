// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The baseline file: its entries, how it is read, and how it is written.

use core::fmt;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Date, GENERATED_REASON, VERSION};
use crate::fingerprint::KEY;

/// One accepted finding: which, why, and until when.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Entry {
    /// The finding's identity: its [`crate::fingerprint`], as SARIF reports it.
    pub fingerprint: String,
    /// Why the finding is accepted, in words. Never blank.
    pub reason: String,
    /// The last day (UTC) the entry accepts the finding. From the next day it
    /// is *lapsed*: it accepts nothing, and the finding counts again.
    pub expires: Option<Date>,
}

impl Entry {
    /// An entry accepting `fingerprint` for `reason`, with no expiry.
    #[must_use]
    pub fn new(fingerprint: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            fingerprint: fingerprint.into(),
            reason: reason.into(),
            expires: None,
        }
    }

    /// This entry, accepting through `date` and no longer.
    #[must_use]
    pub const fn expiring(mut self, date: Date) -> Self {
        self.expires = Some(date);
        self
    }

    /// Whether the entry has stopped accepting anything as of `today`.
    #[must_use]
    pub fn lapsed(&self, today: Date) -> bool {
        self.expires.is_some_and(|expires| expires < today)
    }

    /// Whether the reason is the sentence [`GENERATED_REASON`]: written by a
    /// machine, and not yet replaced by a person.
    #[must_use]
    pub fn unreviewed(&self) -> bool {
        self.reason == GENERATED_REASON
    }
}

/// A set of accepted findings, read from or written to a baseline file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Baseline {
    entries: Vec<Entry>,
}

/// What is wrong with one entry of a baseline file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EntryProblem {
    /// `fingerprint` is empty.
    NoFingerprint,
    /// `reason` is empty or only whitespace.
    NoReason,
    /// `expires` is not a calendar date written `YYYY-MM-DD`.
    BadDate(super::DateError),
    /// The fingerprint is already the one of entry `first` (counting from 1).
    Duplicate {
        /// The earlier entry carrying the same fingerprint.
        first: usize,
    },
}

/// Why a baseline file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BaselineError {
    /// Not JSON of the shape [`JSON_SCHEMA`](super::JSON_SCHEMA) describes, or
    /// carrying a member it does not know.
    Malformed(String),
    /// A `version` this build does not read.
    Version(u32),
    /// A `fingerprint_key` other than the one this build computes: the file
    /// was written for another definition of a finding's identity, and
    /// matching it would report every entry stale.
    Scheme(String),
    /// An entry has an `expires` and the caller did not say what day it is:
    /// applying the baseline would accept for ever what was meant to lapse.
    NoDate,
    /// An entry is not acceptable.
    Entry {
        /// Its position in `findings`, counting from 1.
        index: usize,
        /// The start of its fingerprint, to find it by.
        fingerprint: String,
        /// What is wrong with it.
        problem: EntryProblem,
    },
}

impl fmt::Display for BaselineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "not a baseline file: {why}"),
            Self::Version(found) => write!(
                f,
                "baseline version {found} is not one this build reads (it reads {VERSION})"
            ),
            Self::Scheme(found) => write!(
                f,
                "the baseline holds fingerprints of scheme {found:?}, and this build computes \
                 {KEY:?}; write it again with --write-baseline"
            ),
            Self::NoDate => {
                f.write_str("an entry has an `expires` date, and today's date could not be read")
            }
            Self::Entry {
                index,
                fingerprint,
                problem,
            } => {
                write!(f, "baseline entry {index} ({fingerprint:?}): ")?;
                match problem {
                    EntryProblem::NoFingerprint => f.write_str("`fingerprint` is empty"),
                    EntryProblem::NoReason => {
                        f.write_str("`reason` is empty; say why this finding is accepted, in words")
                    }
                    EntryProblem::BadDate(error) => write!(f, "`expires`: {error}"),
                    EntryProblem::Duplicate { first } => {
                        write!(f, "the same finding as entry {first}")
                    }
                }
            }
        }
    }
}

impl core::error::Error for BaselineError {}

/// A fingerprint cut to a readable length, for an error message.
pub(super) fn shorten(text: &str) -> String {
    const SHOWN: usize = 72;
    let mut shown: String = text.chars().take(SHOWN).collect();
    if text.chars().count() > SHOWN {
        shown.push('…');
    }
    shown
}

/// The file as it is written: members in reading order.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileForm {
    version: u32,
    fingerprint_key: String,
    findings: Vec<EntryForm>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryForm {
    fingerprint: String,
    reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires: Option<String>,
}

impl Baseline {
    /// A baseline of `entries`, in the order given.
    #[must_use]
    pub const fn new(entries: Vec<Entry>) -> Self {
        Self { entries }
    }

    /// The entries, in file order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether any entry carries an `expires`: the only thing that needs
    /// today's date.
    #[must_use]
    pub fn has_expiry(&self) -> bool {
        self.entries.iter().any(|entry| entry.expires.is_some())
    }

    /// Reads a baseline file's text.
    ///
    /// The file is strict on purpose: an unknown member, a blank reason, a
    /// duplicate fingerprint or a malformed date is an error, because a
    /// misspelt `expires` that was ignored would accept a finding forever.
    ///
    /// ```
    /// use mcp_trace_validator::baseline::Baseline;
    ///
    /// let baseline = Baseline::parse(r#"{
    ///     "version": 1,
    ///     "fingerprint_key": "mcpConformanceFinding/v2",
    ///     "findings": [{"fingerprint": "BASE-086:x:y:1", "reason": "the SDK's generator"}]
    /// }"#)?;
    /// assert_eq!(baseline.entries()[0].reason, "the SDK's generator");
    ///
    /// // A reason is the point of the file.
    /// let blank = r#"{"version": 1, "fingerprint_key": "mcpConformanceFinding/v2",
    ///     "findings": [{"fingerprint": "BASE-086:x:y:1", "reason": "  "}]}"#;
    /// assert!(Baseline::parse(blank).is_err());
    /// # Ok::<(), mcp_trace_validator::baseline::BaselineError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`BaselineError`] naming what is wrong and, for an entry, which.
    pub fn parse(text: &str) -> Result<Self, BaselineError> {
        let file: FileForm = serde_json::from_str(text)
            .map_err(|error| BaselineError::Malformed(error.to_string()))?;
        if file.version != VERSION {
            return Err(BaselineError::Version(file.version));
        }
        if file.fingerprint_key != KEY {
            return Err(BaselineError::Scheme(file.fingerprint_key));
        }
        let mut first_of: HashMap<String, usize> = HashMap::new();
        let mut entries = Vec::with_capacity(file.findings.len());
        for (position, form) in file.findings.into_iter().enumerate() {
            let index = position + 1;
            let fail = |fingerprint: &str, problem| BaselineError::Entry {
                index,
                fingerprint: shorten(fingerprint),
                problem,
            };
            if form.fingerprint.is_empty() {
                return Err(fail(&form.fingerprint, EntryProblem::NoFingerprint));
            }
            if form.reason.trim().is_empty() {
                return Err(fail(&form.fingerprint, EntryProblem::NoReason));
            }
            let expires = match form.expires.as_deref().map(str::parse::<Date>) {
                None => None,
                Some(Ok(date)) => Some(date),
                Some(Err(error)) => {
                    return Err(fail(&form.fingerprint, EntryProblem::BadDate(error)));
                }
            };
            if let Some(&first) = first_of.get(&form.fingerprint) {
                return Err(fail(&form.fingerprint, EntryProblem::Duplicate { first }));
            }
            first_of.insert(form.fingerprint.clone(), index);
            entries.push(Entry {
                fingerprint: form.fingerprint,
                reason: form.reason,
                expires,
            });
        }
        Ok(Self { entries })
    }

    /// The file's text: pretty-printed, one trailing newline, entries sorted by
    /// fingerprint so a re-recorded trace or a different run order yields the
    /// same bytes, and a diff of the file shows only what changed.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut findings: Vec<EntryForm> = self
            .entries
            .iter()
            .map(|entry| EntryForm {
                fingerprint: entry.fingerprint.clone(),
                reason: entry.reason.clone(),
                expires: entry.expires.map(|date| date.to_string()),
            })
            .collect();
        findings.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
        let file = FileForm {
            version: VERSION,
            fingerprint_key: KEY.to_owned(),
            findings,
        };
        let mut text = serde_json::to_string_pretty(&file).unwrap_or_default();
        text.push('\n');
        text
    }
}
