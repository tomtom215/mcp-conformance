// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The binary's traces: refused early when they cannot be written, created only
//! once a session can start, and summarized at the end.
//!
//! A trace file is the last thing a run creates. A run that fails to start — a
//! server that will not spawn, an address that will not bind, an upstream that is
//! not a URL — leaves no file behind: an empty one would read as a failed capture,
//! and would make the next run without `--force` refuse to start.
//!
//! `-o` names one file, or — with `{session}` in its file name — a file per
//! session ([`Numbered`]): the stdio wrapper takes the next number each launch, and
//! the HTTP proxy one per client session it carries.

use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcp_conformance_core::trace::{DEFAULT_MAX_LINE_BYTES, LINE_ENVELOPE_BYTES};
use mcp_trace_capture::numbered::{Numbered, PLACEHOLDER};
use mcp_trace_capture::recorder::Sessions;
use mcp_trace_capture::traces::Traces;
use mcp_trace_capture::{Recorder, Summary};

/// The exit code for a session whose trace is incomplete, when it would otherwise
/// be 0.
const EXIT_INCOMPLETE: i32 = 3;

/// Where traces go: the one file `-o` names, or a numbered file per session.
#[derive(Debug)]
pub enum Target {
    /// One file, written as named.
    File(PathBuf),
    /// `{session}` in the file name: the next number per session.
    Numbered(Numbered),
}

/// Refuses an output path before anything is started.
///
/// Refused: `-` (which names a file, not stdout), an existing file without
/// `--force`, a misplaced `{session}`, and a numbered path whose directory does
/// not exist.
///
/// # Errors
///
/// Why the path cannot be used, for the user.
pub fn check(path: &Path, force: bool) -> Result<Target, String> {
    if path.as_os_str() == "-" {
        return Err(
            "-o - would create a file named `-`, not write to stdout (in stdio mode stdout \
             carries the session itself); give the trace a path"
                .to_owned(),
        );
    }
    if let Some(numbered) = Numbered::parse(path)? {
        // Numbered files are never overwritten, so `--force` has nothing to allow.
        if !numbered.dir().is_dir() {
            return Err(format!(
                "{}: {} is not a directory; create it first",
                path.display(),
                numbered.dir().display()
            ));
        }
        return Ok(Target::Numbered(numbered));
    }
    if let Some(warning) = stray_braces(path) {
        eprintln!("mcp-trace-capture: warning: {warning}");
    }
    if !force && path.symlink_metadata().is_ok() {
        return Err(exists(path));
    }
    Ok(Target::File(path.to_path_buf()))
}

/// A warning for a file name with a `{…}` in it that is not [`PLACEHOLDER`] —
/// `{sesion}`, say — which would otherwise be written as a literal name.
fn stray_braces(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let open = name.find('{')?;
    name[open..].contains('}').then(|| {
        format!(
            "{name} has a {{…}} that is not {PLACEHOLDER}, so it is the file's literal name; \
             {PLACEHOLDER} numbers a trace per session"
        )
    })
}

fn exists(path: &Path) -> String {
    format!(
        "{} exists; appending would break the trace's sequence numbers (use --force to \
         overwrite, -o for another path, or {PLACEHOLDER} in the file name for a new file \
         each run)",
        path.display()
    )
}

/// Creates the one trace a stdio session writes — the named file (truncating it
/// under `force`), or the next numbered one — and its recorder.
///
/// # Errors
///
/// Why the file could not be created, for the user.
pub fn open(
    target: &Target,
    force: bool,
    max_message: usize,
) -> Result<(Arc<Recorder>, PathBuf), String> {
    let (path, file) = match target {
        Target::File(path) => (path.clone(), create(path, force)?),
        Target::Numbered(numbered) => {
            let (_, path, file) = numbered.create_next(1).map_err(|error| error.to_string())?;
            (path, file)
        }
    };
    eprintln!("mcp-trace-capture: recording to {}", path.display());
    let max_line = max_line(max_message);
    Ok((
        Arc::new(Recorder::with_max_line(BufWriter::new(file), max_line)),
        path,
    ))
}

/// The proxy's traces: every session into the named file, or each into its own.
/// Returns the named file's path, for the summary.
///
/// # Errors
///
/// Why the named file could not be created, for the user.
pub fn traces(
    target: &Target,
    force: bool,
    max_message: usize,
) -> Result<(Arc<Traces>, Option<PathBuf>), String> {
    match target {
        Target::File(_) => {
            let (recorder, path) = open(target, force, max_message)?;
            Ok((Arc::new(Traces::one(recorder)), Some(path)))
        }
        Target::Numbered(numbered) => {
            eprintln!(
                "mcp-trace-capture: recording a trace per session, numbered after any already \
                 there, like {}",
                numbered.path(1).display()
            );
            Ok((
                Arc::new(Traces::per_session(numbered.clone(), max_line(max_message))),
                None,
            ))
        }
    }
}

fn create(path: &Path, force: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            exists(path)
        } else {
            format!("cannot create {}: {error}", path.display())
        }
    })
}

/// The line limit for `max_message`, saying so when the validator's default
/// would not read it back.
fn max_line(max_message: usize) -> usize {
    let max_line = max_message.saturating_add(LINE_ENVELOPE_BYTES);
    if max_line > DEFAULT_MAX_LINE_BYTES {
        eprintln!(
            "mcp-trace-capture: lines may exceed the validator's default limit; judge \
             this trace with `mcp-trace-validator validate --max-line-bytes {max_line}`"
        );
    }
    max_line
}

/// Flushes a stdio session's trace, reports it, and returns the exit code: `code`,
/// or [`EXIT_INCOMPLETE`] in place of a 0 when a write failed. `ended` is how
/// the server exited, when that is known.
pub fn finish(
    recorder: &Recorder,
    path: &Path,
    code: i32,
    ended: Option<std::process::ExitStatus>,
) -> i32 {
    report(&recorder.finish(), Some(path), code, Hint::None, ended)
}

/// Flushes every trace the proxy wrote and reports each; the exit code is
/// [`EXIT_INCOMPLETE`] in place of a 0 when any is incomplete.
pub fn finish_traces(traces: &Traces, path: Option<&Path>, code: i32) -> i32 {
    let finished = traces.finish();
    if traces.is_per_session() && finished.is_empty() {
        eprintln!("mcp-trace-capture: no session began; no trace was written");
        return code;
    }
    let hint = if traces.is_per_session() {
        Hint::None
    } else {
        Hint::Placeholder
    };
    finished.iter().fold(code, |code, trace| {
        report(
            &trace.summary,
            trace.path.as_deref().or(path),
            code,
            hint,
            None,
        )
    })
}

/// What the multi-session warning suggests besides recording one client per run.
#[derive(Debug, Clone, Copy)]
enum Hint {
    None,
    /// `{session}` would have split this trace: the proxy, writing one file.
    Placeholder,
}

fn report(
    summary: &Summary,
    path: Option<&Path>,
    code: i32,
    hint: Hint,
    ended: Option<std::process::ExitStatus>,
) -> i32 {
    let shown = path.map_or_else(
        || "a session's trace, whose file could not be created,".to_owned(),
        |path| path.display().to_string(),
    );
    if let Some(error) = &summary.error {
        eprintln!(
            "mcp-trace-capture: the trace at {shown} is incomplete: {} event(s) recorded, {} \
             lost after a write failed ({error})",
            summary.recorded, summary.dropped
        );
        warn_about_sessions(&summary.sessions, hint);
        return if code == 0 { EXIT_INCOMPLETE } else { code };
    }
    warn_about_sessions(&summary.sessions, hint);
    if summary.after_close > 0 {
        eprintln!(
            "mcp-trace-capture: {} event(s) came after the trace was closed at the request \
             to stop, and are not in it",
            summary.after_close
        );
    }
    if summary.messages == 0 {
        // A trace of transport events alone is a session that never happened;
        // the validator would refuse it as judging nothing.
        let what = ended.map_or_else(
            || "no message was exchanged".to_owned(),
            |status| format!("the server exited ({status}) before any message was exchanged"),
        );
        eprintln!(
            "mcp-trace-capture: {what}; the trace at {shown} holds {} transport event(s) \
             and nothing to validate",
            summary.recorded
        );
        return code;
    }
    let word = path.map_or_else(
        || "<trace>".to_owned(),
        |path| shell_quoted(&path.display().to_string()),
    );
    eprintln!(
        "mcp-trace-capture: recorded {} event(s) to {shown}; validate with \
         `mcp-trace-validator validate {word}`",
        summary.recorded
    );
    code
}

/// `text` as one word for a POSIX shell: as it is when every character is
/// safe unquoted, else in single quotes (a quote inside written `'\''`).
fn shell_quoted(text: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_./,:@%+=-".contains(c);
    if !text.is_empty() && text.chars().all(safe) {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// Says so when a trace holds more than one session, which the validator would
/// judge as one.
fn warn_about_sessions(sessions: &Sessions, hint: Hint) {
    let count = sessions.count();
    if count > 1 {
        let advice = match hint {
            Hint::None => "record one client per capture run".to_owned(),
            Hint::Placeholder => format!(
                "put {PLACEHOLDER} in -o for a trace per session, or record one client per \
                 capture run"
            ),
        };
        eprintln!(
            "mcp-trace-capture: warning: this trace holds {count} sessions ({} initialize \
             request(s), {} session ID(s)); mcp-trace-validator judges a trace as one \
             session, so their ids and ordering mix — {advice}",
            sessions.initialize_requests,
            sessions.session_ids.len()
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_suggested_path_is_one_shell_word() {
        assert_eq!(shell_quoted("traces/s-001.jsonl"), "traces/s-001.jsonl");
        assert_eq!(shell_quoted("C:/x_y,z@1%+=.jsonl"), "C:/x_y,z@1%+=.jsonl");
        assert_eq!(
            shell_quoted("dir with space/t.jsonl"),
            "'dir with space/t.jsonl'"
        );
        assert_eq!(shell_quoted("it's;$x"), r"'it'\''s;$x'");
        assert_eq!(shell_quoted("a*b"), "'a*b'");
        assert_eq!(shell_quoted(""), "''");
    }

    #[test]
    fn a_misspelt_placeholder_is_warned_about() {
        let warning = stray_braces(Path::new("t/{sesion}.jsonl")).unwrap();
        assert!(warning.starts_with("{sesion}.jsonl has a {…} that is not {session}"));
        assert_eq!(stray_braces(Path::new("t/{x.jsonl")), None);
        assert_eq!(stray_braces(Path::new("t/x}{.jsonl")), None);
        assert_eq!(
            stray_braces(Path::new("{x}/t.jsonl")),
            None,
            "only the file name"
        );
        assert_eq!(stray_braces(Path::new("t.jsonl")), None);
    }

    #[test]
    fn an_existing_file_is_refused_for_what_it_is_and_others_say_why() {
        let dir =
            std::env::temp_dir().join(format!("mcp-trace-capture-output-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("trace.jsonl");
        std::fs::write(&existing, "keep\n").unwrap();
        let error = create(&existing, false).unwrap_err();
        assert!(error.contains("exists; appending"), "{error}");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep\n");
        let missing = dir.join("absent").join("trace.jsonl");
        let error = create(&missing, false).unwrap_err();
        assert!(error.starts_with("cannot create "), "{error}");
        assert!(create(&existing, true).is_ok());
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "");
        std::fs::remove_dir_all(&dir).ok();
    }
}
