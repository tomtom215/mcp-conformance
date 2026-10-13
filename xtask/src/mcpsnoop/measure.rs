// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The measurement: every corpus trace converted, checked by the pinned
//! `mcpsnoop`, and set beside our committed report for it.
//!
//! Our side is read from the golden reports rather than recomputed: the golden
//! suite already proves on every `cargo test` that each golden is exactly what
//! the validator says about its trace, and reading them keeps this task free of
//! any second opinion about which revision judges which directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mcp_trace_validator::reader;
use serde_json::Value;

use super::convert::{CONNECTION, Frame, convert};
use super::data::{Finding, NativeCheck, Ours, Results, Snoop, TraceResult};

/// Every signal that reads the wire, so the exit status means "anything at
/// all". Tool-definition advice (`drift`, `loosened`, `schema`,
/// `annotations`) compares sessions against a baseline or grades style, and is
/// not a conformance reading.
pub(crate) const SIGNALS: &str =
    "error,invalid,warn,mismatch,pending,late-result,duplicate,deprecated,incomplete";

/// The signals whose own descriptions call them protocol violations: a frame
/// that is not JSON-RPC, a broken MCP or JSON-RPC expectation, a routing
/// header disagreeing with the body. The rest — a call that returned an error,
/// one left unanswered when the capture ended, a deprecated feature — say
/// nothing about conformance, by mcpsnoop's own account of them.
pub(crate) const CONFORMANCE: [&str; 3] = ["invalid", "warn", "mismatch"];

/// The corpus directories, in the order the page lists them.
pub(crate) const DIRECTORIES: [&str; 5] = [
    "good",
    "violations",
    "draft/good",
    "draft/violations",
    "draft/captured",
];

/// Runs the binary over the whole corpus.
pub(crate) fn measure(root: &Path, binary: &Path, version: &str) -> Result<Results, String> {
    let scratch = std::env::temp_dir().join(format!("xtask-mcpsnoop-{}", std::process::id()));
    fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let native = native_checks(root, binary, &scratch.join("native"));
    let measured = measure_in(root, binary, &scratch);
    if let Err(error) = fs::remove_dir_all(&scratch) {
        eprintln!(
            "xtask: mcpsnoop — could not remove {}: {error}",
            scratch.display()
        );
    }
    Ok(Results {
        mcpsnoop: version.to_owned(),
        native: native?,
        traces: measured?,
    })
}

/// The conversion held against mcpsnoop's own recordings of the same sessions.
///
/// Each pair under `conformance/mcpsnoop/native/` is one session recorded by
/// `mcp-trace-capture` and by mcpsnoop at once (`<name>.trace.jsonl`,
/// `<name>.mcpsnoop.jsonl`). The converted trace must draw exactly the findings
/// mcpsnoop's capture draws once its frames share one connection — the one
/// field the conversion fixes rather than carries. What the capture *as
/// recorded* draws beyond that is kept, because it is a finding in its own
/// right.
fn native_checks(root: &Path, binary: &Path, scratch: &Path) -> Result<Vec<NativeCheck>, String> {
    let directory = root.join("conformance/mcpsnoop/native");
    let mut checks = Vec::new();
    for trace in trace_files(&directory)? {
        let name = trace
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(session) = name.strip_suffix(".trace.jsonl") else {
            continue;
        };
        let read = |path: &Path| {
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
        };
        let events = reader::parse_trace(&read(&trace)?, &reader::Limits::default())
            .map_err(|error| format!("native/{name}: {error}"))?;
        let converted: Vec<Value> = convert(session, &events).lines;
        let recorded: Vec<Value> = read(&directory.join(format!("{session}.mcpsnoop.jsonl")))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("native/{session}.mcpsnoop.jsonl: {error}"))?;
        let one_connection: Vec<Value> = recorded
            .iter()
            .cloned()
            .map(|mut line| {
                if let Some(connection) = line.get_mut("conn_id") {
                    *connection = Value::from(CONNECTION);
                }
                line
            })
            .collect();
        let ours = messages(
            binary,
            &scratch.join(format!("{session}-converted")),
            &converted,
        )?;
        let theirs = messages(
            binary,
            &scratch.join(format!("{session}-native")),
            &one_connection,
        )?;
        let as_recorded = messages(
            binary,
            &scratch.join(format!("{session}-recorded")),
            &recorded,
        )?;
        let only_as_recorded: Vec<String> = as_recorded
            .iter()
            .filter(|message| !theirs.contains(message))
            .cloned()
            .collect();
        checks.push(NativeCheck {
            session: session.to_owned(),
            findings: ours.len(),
            same_as_native: ours == theirs,
            only_as_recorded,
        });
    }
    Ok(checks)
}

/// Every finding `mcpsnoop check` raises on `lines`, as sorted
/// `signal: message` text. Baseline signals are left out: they compare a
/// session with an earlier one, and each check here starts from none.
fn messages(binary: &Path, scratch: &Path, lines: &[Value]) -> Result<Vec<String>, String> {
    fs::create_dir_all(scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let file = scratch.join("capture.jsonl");
    fs::write(&file, jsonl(lines)).map_err(|error| format!("{}: {error}", file.display()))?;
    let (_, sarif) = check(binary, scratch, &file)?;
    let mut out: Vec<String> = sarif
        .pointer("/runs/0/results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| {
            let rule = result.get("ruleId")?.as_str()?.strip_prefix("mcpsnoop/")?;
            let message = result.pointer("/message/text")?.as_str()?;
            (!matches!(rule, "drift" | "loosened"))
                .then(|| format!("{rule}: {}", strip_prefix(message)))
        })
        .collect();
    out.sort();
    Ok(out)
}

fn jsonl(lines: &[Value]) -> String {
    let mut body = String::new();
    for line in lines {
        body.push_str(&line.to_string());
        body.push('\n');
    }
    body
}

fn measure_in(root: &Path, binary: &Path, scratch: &Path) -> Result<Vec<TraceResult>, String> {
    let mut traces = Vec::new();
    for directory in DIRECTORIES {
        for path in trace_files(&root.join("corpus").join(directory))? {
            let index = traces.len();
            traces.push(measure_one(
                root,
                binary,
                &scratch.join(index.to_string()),
                directory,
                &path,
            )?);
        }
    }
    Ok(traces)
}

fn trace_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    files.sort();
    Ok(files)
}

fn measure_one(
    root: &Path,
    binary: &Path,
    scratch: &Path,
    directory: &str,
    path: &Path,
) -> Result<TraceResult, String> {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = format!("{directory}/{stem}");
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let events = reader::parse_trace(&text, &reader::Limits::default())
        .map_err(|error| format!("{name}: {error}"))?;
    let capture = convert(&name, &events);
    fs::create_dir_all(scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let file = scratch.join("capture.jsonl");
    fs::write(&file, jsonl(&capture.lines))
        .map_err(|error| format!("{}: {error}", file.display()))?;
    let (exit, sarif) = check(binary, scratch, &file)?;
    let mcpsnoop =
        read_sarif(&sarif, &capture.frames, exit).map_err(|error| format!("{name}: {error}"))?;
    let draft = directory.starts_with("draft");
    let golden = root
        .join("corpus/golden")
        .join(if draft { "draft" } else { "" })
        .join(format!("{stem}.json"));
    let (revision, ours) = read_golden(&golden)?;
    let http = capture
        .lines
        .first()
        .and_then(|meta| meta.get("transport"))
        .and_then(Value::as_str)
        == Some("http");
    Ok(TraceResult {
        trace: name,
        revision,
        transport: if http { "http" } else { "stdio" }.to_owned(),
        frames: capture.frames.len(),
        unframed: capture.frames.iter().filter(|frame| !frame.framed).count(),
        ours,
        mcpsnoop,
    })
}

/// `mcpsnoop check` on one capture, in a state directory of its own so no
/// trusted-tool baseline or session history carries from one trace to the next.
fn check(binary: &Path, scratch: &Path, file: &Path) -> Result<(i32, Value), String> {
    let output = Command::new(binary)
        .args([
            "check",
            "--format",
            "sarif",
            "--fail-on",
            SIGNALS,
            "--baseline",
        ])
        .arg(scratch.join("baseline"))
        .arg(file)
        .env("XDG_STATE_HOME", scratch)
        .env("XDG_CONFIG_HOME", scratch)
        .env("XDG_DATA_HOME", scratch)
        .env("XDG_CACHE_HOME", scratch)
        .output()
        .map_err(|error| format!("cannot run {}: {error}", binary.display()))?;
    let sarif = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "mcpsnoop check printed no SARIF for {} ({error}): {}",
            file.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok((output.status.code().unwrap_or(-1), sarif))
}

/// A finding's message without the `session <id> frame <n>: ` prefix.
pub(crate) fn strip_prefix(message: &str) -> &str {
    message
        .strip_prefix("session ")
        .and_then(|rest| rest.split_once(": "))
        .map_or(message, |(_, text)| text)
}

/// The findings of one SARIF run, sorted, each tied back to its trace event.
pub(crate) fn read_sarif(sarif: &Value, frames: &[Frame], exit: i32) -> Result<Snoop, String> {
    let results = sarif
        .pointer("/runs/0/results")
        .and_then(Value::as_array)
        .ok_or("the SARIF report has no runs[0].results")?;
    let mut findings = Vec::new();
    let mut other: BTreeMap<String, usize> = BTreeMap::new();
    for result in results {
        let rule = result
            .get("ruleId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let signal = rule.strip_prefix("mcpsnoop/").unwrap_or(rule);
        if !CONFORMANCE.contains(&signal) {
            *other.entry(signal.to_owned()).or_default() += 1;
            continue;
        }
        let message = result
            .pointer("/message/text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let frame = result
            .pointer("/locations/0/physicalLocation/region/startLine")
            .and_then(Value::as_u64)
            .and_then(|line| usize::try_from(line).ok())
            .and_then(|line| line.checked_sub(1))
            .and_then(|index| frames.get(index));
        findings.push(Finding {
            signal: signal.to_owned(),
            seq: frame.and_then(|frame| frame.seq),
            framed: frame.is_none_or(|frame| frame.framed),
            message: strip_prefix(message).to_owned(),
        });
    }
    findings.sort();
    Ok(Snoop {
        exit,
        findings,
        other,
    })
}

/// The revision and our flagged rows, from a committed golden report.
pub(crate) fn read_golden(path: &Path) -> Result<(String, Ours), String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let golden: Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let field = |name: &str| {
        golden
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut flagged: Vec<String> = golden
        .get("requirements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| {
            matches!(
                row.get("outcome").and_then(Value::as_str),
                Some("fail" | "warn")
            )
        })
        .filter_map(|row| row.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    if golden
        .pointer("/wire_schema/outcome")
        .and_then(Value::as_str)
        == Some("fail")
    {
        flagged.push(super::WIRE_SCHEMA.to_owned());
    }
    flagged.sort();
    Ok((
        field("revision"),
        Ours {
            verdict: field("verdict"),
            flagged,
        },
    ))
}
