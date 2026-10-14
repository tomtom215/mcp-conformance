// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Test libraries: fresh copies of the committed fixture library the
//! screenshot suite serves (`visual/fixtures/library`), so the Rust tests and
//! the browser tests read the same recordings.

#![allow(clippy::unwrap_used)]

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::state::Workbench;

/// The committed fixture library.
pub(crate) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../visual/fixtures/library")
}

/// A scratch directory, removed with everything in it when dropped: a test
/// leaves nothing behind in the temporary directory. (Each one is a copy of
/// the fixture library; a mutation run executes the suite once per mutant, so
/// copies that outlived their test filled CI runners' disks.)
#[derive(Debug)]
pub(crate) struct Scratch(PathBuf);

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// A new, empty scratch directory named for `name`.
pub(crate) fn scratch(name: &str) -> Scratch {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mcp-workbench-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

/// A copy of the fixture library in a scratch directory.
pub(crate) fn library(name: &str) -> Scratch {
    let dir = scratch(name);
    for file in std::fs::read_dir(fixtures()).unwrap() {
        let path = file.unwrap().path();
        std::fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
    }
    dir
}

/// The fixture library, opened.
pub(crate) fn workbench(name: &str) -> (Workbench, Scratch) {
    let dir = library(name);
    (Workbench::open(&dir).unwrap(), dir)
}
