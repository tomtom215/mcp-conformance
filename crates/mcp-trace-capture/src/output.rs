// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The binary's trace file: refused early when it cannot be written, created only
//! once the session can start, and summarized at the end.
//!
//! The file is the last thing a run creates. A run that fails to start — a server
//! that will not spawn, an address that will not bind, an upstream that is not a
//! URL — leaves no file behind: an empty one would read as a failed capture, and
//! would make the next run without `--force` refuse to start.

use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::Path;
use std::sync::Arc;

use mcp_conformance_core::trace::{DEFAULT_MAX_LINE_BYTES, LINE_ENVELOPE_BYTES};
use mcp_trace_capture::Recorder;
use mcp_trace_capture::recorder::Sessions;

/// The exit code for a session whose trace is incomplete, when it would otherwise
/// be 0.
const EXIT_INCOMPLETE: u8 = 3;

/// Refuses an output path before anything is started: `-` (which names a file,
/// not stdout), and an existing file without `--force`.
///
/// # Errors
///
/// Why the path cannot be used, for the user.
pub fn check(path: &Path, force: bool) -> Result<(), String> {
    if path.as_os_str() == "-" {
        return Err(
            "-o - would create a file named `-`, not write to stdout (in stdio mode stdout \
             carries the session itself); give the trace a path"
                .to_owned(),
        );
    }
    if !force && path.symlink_metadata().is_ok() {
        return Err(exists(path));
    }
    Ok(())
}

fn exists(path: &Path) -> String {
    format!(
        "{} exists; appending would break the trace's sequence numbers (use --force to \
         overwrite, or -o for another path)",
        path.display()
    )
}

/// Creates the trace file (truncating it under `force`) and its recorder.
///
/// # Errors
///
/// Why the file could not be created, for the user.
pub fn open(path: &Path, force: bool, max_message: usize) -> Result<Arc<Recorder>, String> {
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            exists(path)
        } else {
            format!("cannot create {}: {error}", path.display())
        }
    })?;
    eprintln!("mcp-trace-capture: recording to {}", path.display());
    let max_line = max_message.saturating_add(LINE_ENVELOPE_BYTES);
    if max_line > DEFAULT_MAX_LINE_BYTES {
        eprintln!(
            "mcp-trace-capture: lines may exceed the validator's default limit; judge \
             this trace with `mcp-trace-validator validate --max-line-bytes {max_line}`"
        );
    }
    Ok(Arc::new(Recorder::with_max_line(
        BufWriter::new(file),
        max_line,
    )))
}

/// Flushes the trace, reports it, and returns the exit code: `code`, or
/// [`EXIT_INCOMPLETE`] in place of a 0 when a write failed.
pub fn finish(recorder: &Recorder, path: &Path, code: u8) -> u8 {
    let summary = recorder.finish();
    if let Some(error) = &summary.error {
        eprintln!(
            "mcp-trace-capture: the trace at {} is incomplete: {} event(s) recorded, {} lost \
             after a write failed ({error})",
            path.display(),
            summary.recorded,
            summary.dropped
        );
        warn_about_sessions(&summary.sessions);
        return if code == 0 { EXIT_INCOMPLETE } else { code };
    }
    warn_about_sessions(&summary.sessions);
    eprintln!(
        "mcp-trace-capture: recorded {} event(s) to {}; validate with \
         `mcp-trace-validator validate {}`",
        summary.recorded,
        path.display(),
        path.display()
    );
    code
}

/// Says so when the trace holds more than one session, which the validator would
/// judge as one.
fn warn_about_sessions(sessions: &Sessions) {
    let count = sessions.count();
    if count > 1 {
        eprintln!(
            "mcp-trace-capture: warning: this trace holds {count} sessions ({} initialize \
             request(s), {} session ID(s)); mcp-trace-validator judges a trace as one \
             session, so their ids and ordering mix — record one client per capture run",
            sessions.initialize_requests,
            sessions.session_ids.len()
        );
    }
}
