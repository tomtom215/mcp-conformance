// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Delivering the binary's output to stdout.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

/// Writes `text` to stdout, returning whether the output was delivered. A reader
/// that closed the pipe early (`… | head`) has what it wanted; that ends the
/// output, it is not an error — `print!` would panic. Any other failure (a full
/// disk, a revoked file) means the report a CI step will upload is truncated or
/// missing, so the caller exits `2` rather than letting the verdict's `0` pass it.
#[must_use]
pub(crate) fn emit(text: &str) -> bool {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => true,
        Err(error) => {
            eprintln!("error: cannot write output: {error}");
            false
        }
    }
}
