// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Files and directories the workbench creates: private to their owner.
//!
//! A library holds recordings, and a recording holds whatever its session
//! carried, credentials and secrets in payloads among it. The recorder makes
//! every trace mode `0600` for that reason. The workbench writes the files
//! *derived* from a recording: an import, a redacted copy, a sidecar (whose
//! cached verdict quotes payload values in its findings), an export. Left at
//! the umask's usual `0644`, each is readable by other users beside the `0600`
//! trace it came from.
//!
//! The rule is two-sided. What the workbench **creates** is private: files
//! `0600` and directories `0700`, less the umask, which can only narrow them.
//! What it **replaces** keeps the mode its owner gave it, so a library shared
//! with a group on purpose keeps working, and the tool never changes a mode
//! silently. A library that already holds readable files is said so once
//! ([`warn_once_if_shared`]), with the command that fixes it.
//!
//! Unix mode bits only. Windows access control is not set here.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Once;

/// Options for a file the workbench creates: writable, and on Unix mode
/// `0600` (less the umask) when it is created. A file that already exists
/// keeps its own mode, since a mode applies only at creation.
#[must_use]
pub(crate) fn file() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
}

/// [`std::fs::create_dir_all`], with each directory it creates `0700` (less
/// the umask) on Unix. A directory that exists is not touched.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Writes `bytes` to `path`: a new file is private, and an existing one is
/// truncated and keeps its mode.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    file()
        .create(true)
        .truncate(true)
        .open(path)?
        .write_all(bytes)
}

/// The recordings and sidecars directly in `dir` that group or other can
/// read, sorted. Empty off Unix, which has no such bits to read.
#[cfg(unix)]
#[must_use]
pub(crate) fn shared_files(dir: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut shared: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".jsonl") || name.ends_with(".meta.json")
        })
        .filter_map(|entry| Some((entry.path(), entry.metadata().ok()?)))
        .filter(|(_, metadata)| metadata.is_file() && metadata.permissions().mode() & 0o044 != 0)
        .map(|(path, _)| path)
        .collect();
    shared.sort();
    shared
}

/// See the Unix version.
#[cfg(not(unix))]
#[must_use]
pub(crate) fn shared_files(_dir: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// Says once per process, on stderr, that `dir` holds recordings or sidecars
/// other users can read, and how to make them private. It changes nothing.
pub(crate) fn warn_once_if_shared(dir: &Path) {
    static SAID: Once = Once::new();
    let shared = shared_files(dir);
    let Some(first) = shared.first() else {
        return;
    };
    SAID.call_once(|| {
        eprintln!(
            "mcp-conformance: {} file(s) in the library {} can be read by other users (for \
             example {}). Recordings can hold credentials; what the workbench writes is for its \
             owner alone, but it does not change the mode of a file that already exists. To make \
             them private: chmod -R go-rwx {}",
            shared.len(),
            dir.display(),
            first.display(),
            dir.display(),
        );
    });
}

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mcp-conformance-private-{name}-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_written_file_is_new_and_private_or_old_and_unchanged_in_mode() {
        let dir = scratch("write");
        let path = dir.join("a.tar");
        write(&path, b"first").unwrap();
        assert_eq!(mode_of(&path) & 0o077, 0, "a new file is open to others");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        write(&path, b"2").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"2", "truncated, not appended");
        assert_eq!(mode_of(&path), 0o640);
    }

    #[test]
    fn only_new_directories_are_made_private() {
        let dir = scratch("dirs");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let made = dir.join("one").join("two");
        create_dir_all(&made).unwrap();
        for path in [dir.join("one"), made.clone()] {
            assert_eq!(mode_of(&path) & 0o077, 0, "{path:?}");
        }
        assert_eq!(
            mode_of(&dir),
            0o755,
            "a directory that existed is left alone"
        );
        create_dir_all(&made).unwrap();
    }

    #[test]
    fn the_shared_files_are_the_recordings_and_sidecars_others_can_read() {
        let dir = scratch("shared");
        for (name, mode) in [
            ("open.jsonl", 0o644),
            ("open.meta.json", 0o604),
            ("group.jsonl", 0o640),
            ("private.jsonl", 0o600),
            ("private.meta.json", 0o600),
            ("notes.txt", 0o644),
        ] {
            fs::write(dir.join(name), "").unwrap();
            fs::set_permissions(dir.join(name), fs::Permissions::from_mode(mode)).unwrap();
        }
        fs::create_dir(dir.join("folder.jsonl")).unwrap();
        let names: Vec<String> = shared_files(&dir)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["group.jsonl", "open.jsonl", "open.meta.json"]);
        assert_eq!(shared_files(&dir.join("absent")), Vec::<PathBuf>::new());
    }
}
