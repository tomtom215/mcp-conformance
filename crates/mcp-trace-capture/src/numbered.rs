// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Numbered trace paths: `-o 'traces/{session}.jsonl'` writes `traces/001.jsonl`,
//! `traces/002.jsonl`, … — one trace per session.
//!
//! A number is claimed by creating its file with `create_new`, which the
//! operating system makes atomic: an existing file is never opened, let alone
//! overwritten, and two captures starting at once (a client relaunching a server,
//! or launching two) cannot both claim the same number. Each takes the number
//! after the highest one present, never a gap an earlier trace left when it was
//! deleted, so a numbered directory reads in session order.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// The placeholder a trace path names its session number with.
pub const PLACEHOLDER: &str = "{session}";

/// How far a search for a free number goes before giving up — far past any
/// real suite, and a bound on a file system that reports every name as taken.
const MAX_NUMBER: u64 = 1_000_000;

/// A trace path with [`PLACEHOLDER`] in its file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Numbered {
    dir: PathBuf,
    prefix: String,
    suffix: String,
}

impl Numbered {
    /// The template `path` describes, or `None` when it has no placeholder (a
    /// plain path, written as it is).
    ///
    /// # Errors
    ///
    /// The placeholder appears more than once, or outside the file name.
    pub fn parse(path: &Path) -> Result<Option<Self>, String> {
        let whole = path.to_string_lossy();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return if whole.contains(PLACEHOLDER) {
                Err(format!(
                    "{}: {PLACEHOLDER} must be in the file name",
                    path.display()
                ))
            } else {
                Ok(None)
            };
        };
        let in_name = name.matches(PLACEHOLDER).count();
        if whole.matches(PLACEHOLDER).count() > in_name {
            return Err(format!(
                "{}: {PLACEHOLDER} must be in the file name, not a directory",
                path.display()
            ));
        }
        match name.split_once(PLACEHOLDER) {
            None => Ok(None),
            Some(_) if in_name > 1 => Err(format!(
                "{}: {PLACEHOLDER} may appear only once",
                path.display()
            )),
            Some((prefix, suffix)) => Ok(Some(Self {
                dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
                prefix: prefix.to_owned(),
                suffix: suffix.to_owned(),
            })),
        }
    }

    /// The path for session `number`, zero-padded to three digits so a listing
    /// sorts in session order up to 999.
    #[must_use]
    pub fn path(&self, number: u64) -> PathBuf {
        self.dir
            .join(format!("{}{number:03}{}", self.prefix, self.suffix))
    }

    /// The directory the traces go in (`.` for a bare file name).
    #[must_use]
    pub fn dir(&self) -> &Path {
        if self.dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &self.dir
        }
    }

    /// Creates the next trace: numbered from `from`, or from one past the
    /// highest number already in the directory if that is later, the first
    /// whose file does not exist yet. Returns its number, path and file.
    ///
    /// # Errors
    ///
    /// A file could not be created for a reason other than its existing, or no
    /// number up to a million was free. The error names the path tried.
    pub fn create_next(&self, from: u64) -> io::Result<(u64, PathBuf, File)> {
        let after_highest = self
            .highest()
            .map_or(1, |highest| highest.saturating_add(1));
        for number in from.max(after_highest).max(1)..=MAX_NUMBER {
            let path = self.path(number);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((number, path, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("cannot create {}: {error}", path.display()),
                    ));
                }
            }
        }
        Err(io::Error::other(format!(
            "no free trace number for {} up to {MAX_NUMBER}",
            self.path(0).display()
        )))
    }

    /// The highest number up to [`MAX_NUMBER`] a file in the directory carries
    /// in this template's place, if any. An unreadable directory has none; creating the file then
    /// says why.
    fn highest(&self) -> Option<u64> {
        std::fs::read_dir(self.dir())
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name();
                let digits = name
                    .to_str()?
                    .strip_prefix(&self.prefix)?
                    .strip_suffix(&self.suffix)?
                    .to_owned();
                if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                digits.parse::<u64>().ok()
            })
            // A number the search would never claim (a hand-named file) is
            // not a session this template numbered; counting from it would
            // leave no number to take.
            .filter(|&number| number <= MAX_NUMBER)
            .max()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Option<Numbered>, String> {
        Numbered::parse(Path::new(text))
    }

    #[test]
    fn a_path_without_the_placeholder_is_plain() {
        assert_eq!(parse("trace.jsonl"), Ok(None));
        assert_eq!(parse("/tmp/n/trace.jsonl"), Ok(None));
    }

    #[test]
    fn the_number_replaces_the_placeholder_zero_padded() {
        let numbered = parse("traces/run-{session}.jsonl").unwrap().unwrap();
        assert_eq!(numbered.path(1), Path::new("traces/run-001.jsonl"));
        assert_eq!(numbered.path(1234), Path::new("traces/run-1234.jsonl"));
        assert_eq!(numbered.dir(), Path::new("traces"));
        let bare = parse("{session}").unwrap().unwrap();
        assert_eq!(bare.path(7), Path::new("007"));
        assert_eq!(bare.dir(), Path::new("."));
    }

    #[test]
    fn the_placeholder_belongs_once_in_the_file_name() {
        assert!(
            parse("{session}/trace.jsonl")
                .unwrap_err()
                .contains("not a directory")
        );
        assert!(
            parse("{session}/{session}.jsonl")
                .unwrap_err()
                .contains("not a directory")
        );
        assert!(
            parse("a-{session}-{session}.jsonl")
                .unwrap_err()
                .contains("only once")
        );
    }

    #[test]
    fn numbers_are_claimed_without_touching_existing_files() {
        let dir =
            std::env::temp_dir().join(format!("mcp-trace-capture-numbered-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let numbered = Numbered::parse(&dir.join("s-{session}.jsonl"))
            .unwrap()
            .unwrap();
        std::fs::write(numbered.path(1), "keep\n").unwrap();
        std::fs::write(numbered.path(3), "keep\n").unwrap();
        // Not this template's: another prefix or suffix, or not a number.
        for other in [
            "x-007.jsonl",
            "s-009.txt",
            "s-.jsonl",
            "s-1a.jsonl",
            "s-+9.jsonl",
        ] {
            std::fs::write(dir.join(other), "").unwrap();
        }
        let (first, path, _) = numbered.create_next(1).unwrap();
        assert_eq!(
            (first, path),
            (4, numbered.path(4)),
            "after the highest, not into the gap at 2"
        );
        let (second, ..) = numbered.create_next(1).unwrap();
        assert_eq!(second, 5);
        let (later, ..) = numbered.create_next(10).unwrap();
        assert_eq!(later, 10, "a later start wins");
        let (next, ..) = numbered.create_next(1).unwrap();
        assert_eq!(next, 11);
        assert_eq!(std::fs::read_to_string(numbered.path(1)).unwrap(), "keep\n");
        assert_eq!(std::fs::read_to_string(numbered.path(3)).unwrap(), "keep\n");
        let missing = Numbered::parse(&dir.join("absent/{session}"))
            .unwrap()
            .unwrap();
        let error = missing.create_next(1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
        assert!(error.to_string().starts_with("cannot create "), "{error}");
        assert!(error.to_string().contains("absent"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A stray file numbered past the search's bound — a hand-named
    /// `s-9999999.jsonl` — put "after the highest" beyond every number the
    /// search may try, so each later trace failed with "no free trace number".
    #[test]
    fn a_number_past_the_bound_is_not_the_highest() {
        let dir = std::env::temp_dir().join(format!(
            "mcp-trace-capture-numbered-stray-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let numbered = Numbered::parse(&dir.join("s-{session}.jsonl"))
            .unwrap()
            .unwrap();
        std::fs::write(numbered.path(2), "").unwrap();
        std::fs::write(numbered.path(MAX_NUMBER + 1), "").unwrap();
        std::fs::write(dir.join("s-99999999999999999999999.jsonl"), "").unwrap();
        let (number, ..) = numbered.create_next(1).unwrap();
        assert_eq!(number, 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
