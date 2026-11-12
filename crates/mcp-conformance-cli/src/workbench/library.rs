// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The library on disk: one directory of JSON Lines traces, each with an
//! optional sidecar.
//!
//! ```text
//! library/
//!   sdk-go-2025-11-25.jsonl        the recording, as the recorder wrote it
//!   sdk-go-2025-11-25.meta.json    tags, provenance, the verdict cache
//! ```
//!
//! An entry's id is its file stem. Ids are restricted to ASCII letters,
//! digits, `.`, `_` and `-` (at most [`MAX_ID`] bytes, not starting with a
//! dot), so an id from a request can never name a path outside the library;
//! a `.jsonl` file whose stem is not such an id is listed as skipped and never
//! opened. Nothing here edits a trace in place: importing and redacting add
//! new entries, deleting removes both files.
//!
//! What the library creates is private to its owner ([`private`]): a recording
//! can hold credentials, and a sidecar quotes payload values.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::meta::Meta;
use super::private;

/// The longest id, in bytes: room for a `.meta.json` suffix inside the
/// 100-byte name field of the tar format an export writes.
pub(crate) const MAX_ID: usize = 80;

/// A library directory.
#[derive(Debug, Clone)]
pub(crate) struct Library {
    root: PathBuf,
}

/// A `.jsonl` file the library does not open, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Skipped {
    pub(crate) file: String,
    pub(crate) reason: String,
}

/// A sidecar as read: the sidecar, and whether it could be read (an
/// unreadable one is never overwritten).
#[derive(Debug, Clone)]
pub(crate) struct ReadMeta {
    pub(crate) meta: Meta,
    pub(crate) problem: Option<String>,
}

/// Whether `id` is an entry id: see the module documentation.
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// An id made from a file name a person gave: its last path component,
/// without a `.jsonl` or `.json` extension, every other character replaced
/// by `-`, cut to fit.
pub(crate) fn id_from_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = base
        .strip_suffix(".jsonl")
        .or_else(|| base.strip_suffix(".json"))
        .unwrap_or(base);
    let mut id: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect();
    id = String::from(id.trim_start_matches('.'));
    id.truncate(MAX_ID - 4);
    if id.is_empty() {
        "session".to_owned()
    } else {
        id
    }
}

impl Library {
    /// The library at `root`, created when it does not exist.
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        private::create_dir_all(root)?;
        private::warn_once_if_shared(root);
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// The directory.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Every entry's id, sorted, and every `.jsonl` file not taken as one.
    pub(crate) fn scan(&self) -> io::Result<(Vec<String>, Vec<Skipped>)> {
        let mut ids = Vec::new();
        let mut skipped = Vec::new();
        for dirent in fs::read_dir(&self.root)? {
            let dirent = dirent?;
            let name = dirent.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".jsonl") else {
                continue;
            };
            if !dirent.file_type()?.is_file() {
                skipped.push(Skipped {
                    file: name,
                    reason: "not a regular file".to_owned(),
                });
            } else if valid_id(stem) {
                ids.push(stem.to_owned());
            } else {
                skipped.push(Skipped {
                    file: name,
                    reason: format!(
                        "the name is not a library id (ASCII letters, digits, '.', '_' and '-', \
                         at most {MAX_ID} bytes, not starting with '.')"
                    ),
                });
            }
        }
        ids.sort();
        skipped.sort_by(|a, b| a.file.cmp(&b.file));
        Ok((ids, skipped))
    }

    /// The trace of entry `id`.
    pub(crate) fn trace_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.jsonl"))
    }

    /// The sidecar of entry `id`.
    pub(crate) fn meta_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.meta.json"))
    }

    /// Entry `id`'s sidecar: an empty one when there is none, and the
    /// reason when one exists but cannot be read.
    pub(crate) fn read_meta(&self, id: &str) -> ReadMeta {
        match fs::read_to_string(self.meta_path(id)) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(meta) => ReadMeta {
                    meta,
                    problem: None,
                },
                Err(error) => ReadMeta {
                    meta: Meta::default(),
                    problem: Some(format!("the sidecar is not valid: {error}")),
                },
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => ReadMeta {
                meta: Meta::default(),
                problem: None,
            },
            Err(error) => ReadMeta {
                meta: Meta::default(),
                problem: Some(format!("the sidecar cannot be read: {error}")),
            },
        }
    }

    /// Writes entry `id`'s sidecar, whole or not at all: a temporary file in
    /// the library, then renamed over the old one. The temporary is created
    /// new under a name of its own (so a link planted at a guessable name is
    /// never followed, and two writers never share one) and private; a sidecar
    /// that is being replaced keeps the mode it had.
    pub(crate) fn write_meta(&self, id: &str, meta: &Meta) -> io::Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = self.root.join(format!(
            ".{id}.meta.json.{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let target = self.meta_path(id);
        let written = private::file()
            .create_new(true)
            .open(&temporary)
            .and_then(|mut file| {
                file.write_all(meta.to_text().as_bytes())?;
                if let Ok(existing) = fs::metadata(&target) {
                    fs::set_permissions(&temporary, existing.permissions())?;
                }
                fs::rename(&temporary, &target)
            });
        if written.is_err() {
            fs::remove_file(&temporary).ok();
        }
        written
    }

    /// Adds a recording under an id made from `name` (made unique with a
    /// `-2`, `-3`, … suffix), with its sidecar; returns the id. The trace
    /// file is created new, never overwritten.
    pub(crate) fn add(&self, name: &str, trace: &[u8], meta: &Meta) -> io::Result<String> {
        let base = id_from_name(name);
        for attempt in 1_u32..10_000 {
            let id = if attempt == 1 {
                base.clone()
            } else {
                format!("{base}-{attempt}")
            };
            if self.meta_path(&id).exists() {
                continue;
            }
            match private::file().create_new(true).open(self.trace_path(&id)) {
                Ok(mut file) => {
                    file.write_all(trace)?;
                    file.sync_all()?;
                    self.write_meta(&id, meta)?;
                    return Ok(id);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(format!("no free id for {base}")))
    }

    /// Removes entry `id`: its trace and its sidecar.
    pub(crate) fn delete(&self, id: &str) -> io::Result<()> {
        fs::remove_file(self.trace_path(id))?;
        match fs::remove_file(self.meta_path(id)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mcp-conformance-library-{name}-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn ids_cannot_name_a_path_outside_the_library() {
        for good in ["a", "sdk-go-2025-11-25", "x.y_z", &"a".repeat(MAX_ID)] {
            assert!(valid_id(good), "{good}");
        }
        for bad in [
            "",
            ".hidden",
            "..",
            "../x",
            "a/b",
            "a\\b",
            "a b",
            "é",
            "a\0",
            &"a".repeat(MAX_ID + 1),
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn names_become_ids() {
        assert_eq!(id_from_name("/tmp/a b/sess ion.jsonl"), "sess-ion");
        assert_eq!(id_from_name("C:\\x\\inspector.json"), "inspector");
        assert_eq!(id_from_name("..jsonl"), "session");
        assert_eq!(id_from_name("...x"), "x");
        assert_eq!(id_from_name(""), "session");
        assert_eq!(id_from_name(&"é".repeat(200)).len(), MAX_ID - 4);
        assert!(valid_id(&id_from_name("<script>.jsonl")));
    }

    #[test]
    fn entries_are_added_listed_and_deleted() {
        let dir = scratch("entries");
        let library = Library::open(&dir).unwrap();
        assert_eq!(library.root(), dir);
        let meta = Meta::new("imported");
        let first = library.add("s.jsonl", b"one\n", &meta).unwrap();
        let second = library.add("s.jsonl", b"two\n", &meta).unwrap();
        assert_eq!((first.as_str(), second.as_str()), ("s", "s-2"));
        fs::write(dir.join("bad name.jsonl"), "").unwrap();
        fs::create_dir(dir.join("dir.jsonl")).unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        let (ids, skipped) = library.scan().unwrap();
        assert_eq!(ids, ["s", "s-2"]);
        let files: Vec<&str> = skipped.iter().map(|s| s.file.as_str()).collect();
        assert_eq!(files, ["bad name.jsonl", "dir.jsonl"]);
        assert_eq!(fs::read(library.trace_path("s-2")).unwrap(), b"two\n");
        assert_eq!(library.read_meta("s").meta.source.kind, "imported");

        library.delete("s").unwrap();
        assert!(!library.trace_path("s").exists());
        assert!(!library.meta_path("s").exists());
        assert!(library.delete("s").is_err(), "a missing entry is an error");
        // A trace without a sidecar deletes too.
        fs::remove_file(library.meta_path("s-2")).unwrap();
        library.delete("s-2").unwrap();
        // An id whose sidecar alone is left over is not reused.
        fs::write(library.meta_path("t"), "{}").unwrap();
        assert_eq!(library.add("t", b"", &meta).unwrap(), "t-2");
    }

    #[test]
    fn a_scan_lists_every_trace_by_id_and_says_why_it_skipped_the_rest() {
        let dir = crate::workbench::fixture::library("library-scan");
        fs::write(dir.join("bad name.jsonl"), "").unwrap();
        fs::create_dir(dir.join("folder.jsonl")).unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        let (ids, skipped) = Library::open(&dir).unwrap().scan().unwrap();
        assert_eq!(
            ids,
            [
                "base-001-request-id-boolean",
                "everything-http",
                "everything-stdio",
                "hostile-markup",
                "leaky-session",
                "sdk-go-2025-11-25",
                "sdk-python-2025-11-25",
                "sdk-rust-2025-11-25",
                "sdk-rust-2026-07-28",
                "sdk-typescript-2025-11-25",
                "tool-005-name-length",
            ]
        );
        let skipped: Vec<(&str, &str)> = skipped
            .iter()
            .map(|s| (s.file.as_str(), s.reason.as_str()))
            .collect();
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert_eq!(skipped[0].0, "bad name.jsonl");
        assert!(skipped[0].1.starts_with("the name is not a library id"));
        assert_eq!(skipped[1], ("folder.jsonl", "not a regular file"));
    }

    #[test]
    fn a_trace_without_a_sidecar_keeps_its_id_and_errors_are_not_retried() {
        let dir = crate::workbench::fixture::scratch("library-taken");
        let library = Library::open(&dir).unwrap();
        // A trace whose sidecar is missing still holds its id.
        fs::write(library.trace_path("u"), "kept\n").unwrap();
        let meta = Meta::new("imported");
        assert_eq!(library.add("u", b"new\n", &meta).unwrap(), "u-2");
        assert_eq!(fs::read(library.trace_path("u")).unwrap(), b"kept\n");
        // A sidecar that cannot be removed is an error, not a deletion.
        fs::create_dir(library.meta_path("u")).unwrap();
        assert!(library.delete("u").is_err());
        // A library that is gone is reported as gone, not as out of ids.
        let gone = Library::open(&dir.join("gone")).unwrap();
        fs::remove_dir(gone.root()).unwrap();
        let error = gone.add("v", b"", &meta).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
    }

    #[test]
    fn an_unreadable_sidecar_is_reported_not_replaced() {
        let dir = scratch("sidecar");
        let library = Library::open(&dir).unwrap();
        assert!(library.read_meta("none").problem.is_none());
        fs::write(library.meta_path("x"), "{not json").unwrap();
        let read = library.read_meta("x");
        assert!(read.problem.unwrap().contains("not valid"));
        fs::create_dir(library.meta_path("d")).unwrap();
        assert!(
            library
                .read_meta("d")
                .problem
                .unwrap()
                .contains("cannot be read")
        );
        let mut meta = Meta::default();
        meta.set_tags(["t"]);
        library.write_meta("y", &meta).unwrap();
        assert_eq!(library.read_meta("y").meta.tags, ["t"]);
    }

    /// The permission bits of `path`.
    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// What the workbench writes is as private as the recorder's own output
    /// (mode `0600`): an import, a redacted copy and a sidecar would otherwise
    /// be left at the umask's `0644` beside a `0600` trace, and a sidecar
    /// quotes payload values. Holds for any umask, as a mode of `0600` less
    /// the umask never gives group or other a bit.
    #[cfg(unix)]
    #[test]
    fn a_new_library_and_what_is_added_to_it_are_owner_only() {
        let root = scratch("private");
        let nested = root.join("made").join("library");
        let library = Library::open(&nested).unwrap();
        for dir in [&nested, &root.join("made")] {
            assert_eq!(mode_of(dir) & 0o077, 0, "{dir:?} is open to others");
        }
        let id = library
            .add("s.jsonl", b"{}\n", &Meta::new("imported"))
            .unwrap();
        for path in [library.trace_path(&id), library.meta_path(&id)] {
            assert_eq!(mode_of(&path) & 0o077, 0, "{path:?} is open to others");
        }
    }

    /// A library its owner deliberately shares with a group keeps working: the
    /// tool never changes the mode of a file it replaces.
    #[cfg(unix)]
    #[test]
    fn replacing_a_sidecar_keeps_the_mode_its_owner_gave_it() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("keep-mode");
        let library = Library::open(&dir).unwrap();
        fs::write(library.meta_path("g"), "{}").unwrap();
        fs::set_permissions(library.meta_path("g"), fs::Permissions::from_mode(0o640)).unwrap();
        let mut meta = Meta::default();
        meta.set_tags(["shared"]);
        library.write_meta("g", &meta).unwrap();
        assert_eq!(mode_of(&library.meta_path("g")), 0o640);
        assert_eq!(library.read_meta("g").meta.tags, ["shared"]);
    }

    /// The sidecar is written through a temporary file whose name is its own:
    /// a link planted at the old fixed name is not followed, and no temporary
    /// is left behind.
    #[cfg(unix)]
    #[test]
    fn a_planted_temporary_name_is_not_followed_and_none_is_left() {
        let dir = scratch("temp-name");
        let library = Library::open(&dir).unwrap();
        let target = dir.join("elsewhere.txt");
        fs::write(&target, "not the sidecar").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(".y.meta.json.tmp")).unwrap();
        library.write_meta("y", &Meta::default()).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "not the sidecar");
        assert!(library.meta_path("y").exists());
        let left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| {
                Path::new(name).extension().is_some_and(|ext| ext == "tmp")
                    && name != ".y.meta.json.tmp"
            })
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }
}
