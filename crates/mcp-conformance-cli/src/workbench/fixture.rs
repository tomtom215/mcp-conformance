// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Test libraries: fresh copies of the committed fixture library the
//! screenshot suite serves (`visual/fixtures/library`), so the Rust tests and
//! the browser tests read the same recordings.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::state::Workbench;

/// The committed fixture library.
pub(crate) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../visual/fixtures/library")
}

/// A new, empty scratch directory named for `name`.
pub(crate) fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mcp-workbench-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A copy of the fixture library in a scratch directory.
pub(crate) fn library(name: &str) -> PathBuf {
    let dir = scratch(name);
    for file in std::fs::read_dir(fixtures()).unwrap() {
        let path = file.unwrap().path();
        std::fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
    }
    dir
}

/// The fixture library, opened.
pub(crate) fn workbench(name: &str) -> (Workbench, PathBuf) {
    let dir = library(name);
    (Workbench::open(&dir).unwrap(), dir)
}
