// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Numbered trace paths: `-o 'traces/{session}.jsonl'` writes `traces/001.jsonl`,
//! `traces/002.jsonl`, … — one trace per session.
//!
//! A number is claimed by creating its file with `create_new`, which the
//! operating system makes atomic: an existing file is never opened, let alone
//! overwritten, and two captures starting at once (a client relaunching a server,
//! or launching two) cannot both claim the same number. Each takes the lowest
//! number free when it looks, so a numbered directory reads in session order.

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

    /// Creates the lowest-numbered trace, from `from` up, whose file does not
    /// exist yet; returns its number, path and file.
    ///
    /// # Errors
    ///
    /// A file could not be created for a reason other than its existing, or no
    /// number up to a million was free. The error names the path tried.
    pub fn create_next(&self, from: u64) -> io::Result<(u64, PathBuf, File)> {
        for number in from.max(1)..=MAX_NUMBER {
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
        let (first, path, _) = numbered.create_next(1).unwrap();
        assert_eq!((first, path), (2, numbered.path(2)));
        let (second, ..) = numbered.create_next(1).unwrap();
        assert_eq!(second, 4, "3 is taken, 2 was just claimed");
        let (later, ..) = numbered.create_next(10).unwrap();
        assert_eq!(later, 10);
        assert_eq!(std::fs::read_to_string(numbered.path(1)).unwrap(), "keep\n");
        assert_eq!(std::fs::read_to_string(numbered.path(3)).unwrap(), "keep\n");
        let missing = Numbered::parse(&dir.join("absent/{session}"))
            .unwrap()
            .unwrap();
        let error = missing.create_next(1).unwrap_err();
        assert!(error.to_string().contains("absent"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
