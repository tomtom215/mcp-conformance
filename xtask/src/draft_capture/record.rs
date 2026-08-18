// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Recording one leg: spawning the two processes, and waiting for the file.
//!
//! Split from [`super`] because this is the part that touches the world —
//! processes, ports, the filesystem — while the parent decides what a leg
//! *means* and how its recording is judged.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{ERROR_BUDGET, LOG_LEVEL, Leg, REVISION, TURN_LIMIT};

/// How long to wait for the tap to stop growing, and how often to look.
///
/// The gap being waited out is one thread hop between answering a request and
/// recording it, so two consecutive unchanged samples is generous; the cap is
/// there so a server that never stops writing fails the task instead of
/// hanging it.
const SETTLE_POLL: std::time::Duration = std::time::Duration::from_millis(50);
const SETTLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Drives one session over `leg`'s transport and returns the trace it wrote.
///
/// stdio spawns the server as a child of the host and takes the host's own
/// recording. The HTTP legs spawn the server first, on an OS-assigned port,
/// and take the *server's* tap instead — the host's `Transport` seam carries
/// no headers by construction, so a host-side HTTP recording would judge the
/// transport clauses on evidence it structurally cannot hold.
pub(super) fn record(root: &Path, results: &Path, leg: Leg) -> Result<PathBuf, String> {
    std::fs::create_dir_all(results)
        .map_err(|error| format!("cannot create {}: {error}", results.display()))?;
    match leg {
        Leg::Stdio => record_stdio(root, results),
        Leg::Http | Leg::Probe => record_http(root, results, leg),
    }
}

/// The freshly built binary of `name`.
fn binary(root: &Path, name: &str) -> String {
    root.join(format!(
        "target/debug/{name}{}",
        std::env::consts::EXE_SUFFIX
    ))
    .display()
    .to_string()
}

/// The host's arguments that define the conforming capture, shared by the
/// stdio and HTTP legs.
///
/// One list rather than two, because the whole value of that pair is that the
/// *session* is the same and only the transport differs; a flag that reached
/// one leg and not the other would make every difference between the two
/// reports ambiguous.
fn session_args() -> Vec<&'static str> {
    vec![
        "--protocol-version",
        REVISION,
        "--error-budget",
        ERROR_BUDGET,
        "--turn-limit",
        TURN_LIMIT,
        // `subscriptions/listen` is the one `2026-07-28` feature no tool call
        // reaches: it is a long-lived request, not a tool, so a sweep of the
        // tool list would record everything about this server except the
        // mechanism the revision introduced to replace `resources/subscribe`.
        "--subscribe",
        // The rest of the surface: prompts, resources, templates, completion,
        // and the one read that draws an error. Without it the recording
        // evidences the tool clauses and almost nothing else.
        "--sweep",
        "--log-level",
        LOG_LEVEL,
    ]
}

/// stdio: the host spawns the server and records at its own transport seam.
fn record_stdio(root: &Path, results: &Path) -> Result<PathBuf, String> {
    let server = format!(
        "{} --transport stdio --protocol-version {REVISION}",
        binary(root, "mcp-everything-server")
    );
    eprintln!("xtask: draft-capture — stdio — host against {server}");
    let status = Command::new(binary(root, "mcp-reference-host"))
        .args(["--server-cmd", &server])
        .args(session_args())
        .arg("--trace-dir")
        .arg(results)
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run the reference host: {error}"))?;
    if !status.success() {
        return Err(format!("the host exited {status}; no capture taken"));
    }
    sole_trace(results)
}

/// HTTP: the server is spawned first and taps its own side of the wire.
///
/// Both HTTP legs come through here because the recording apparatus is the
/// same; only the client's arguments differ, and that difference *is* the two
/// legs — a conforming session versus a deliberately malformed one.
fn record_http(root: &Path, results: &Path, leg: Leg) -> Result<PathBuf, String> {
    let (mut server, address) = crate::conformance::start_server(root, results, REVISION)
        .ok_or_else(|| "the server did not come up on HTTP".to_owned())?;
    let url = format!("http://{address}/mcp");
    eprintln!("xtask: draft-capture — {} — host against {url}", leg.name());
    let mut command = Command::new(binary(root, "mcp-reference-host"));
    command.args(["--url", &url]);
    if leg == Leg::Probe {
        command.arg("--probe");
    } else {
        command.args(session_args());
    }
    let status = command.current_dir(root).status();
    // The client exiting does not mean the server has finished *writing*: it
    // returns the HTTP response and then taps it, so killing on the client's
    // exit races the last line out of the file. That silently truncated the
    // probe capture by one message and moved two clauses' outcomes with it.
    settled(results);
    // Killed either way: a host that failed mid-session must not leave a
    // listener holding the port for the next run.
    let _ = server.kill();
    let _ = server.wait();
    let status = status.map_err(|error| format!("cannot run the reference host: {error}"))?;
    if !status.success() {
        return Err(format!("the host exited {status}; no capture taken"));
    }
    sole_trace(results)
}

/// Blocks until nothing in `results` has changed size for two samples running.
///
/// Deliberately not a fixed sleep: a fixed one is either too short on a loaded
/// machine — which is exactly when this races — or wasted time on an idle one,
/// and neither tells you whether it worked.
fn settled(results: &Path) {
    let sizes = || -> Vec<u64> {
        let mut sizes: Vec<u64> = std::fs::read_dir(results)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok()?.metadata().ok().map(|meta| meta.len()))
            .collect();
        sizes.sort_unstable();
        sizes
    };
    let deadline = std::time::Instant::now() + SETTLE_LIMIT;
    let mut previous = sizes();
    while std::time::Instant::now() < deadline {
        std::thread::sleep(SETTLE_POLL);
        let current = sizes();
        if current == previous && !current.is_empty() {
            return;
        }
        previous = current;
    }
    eprintln!(
        "xtask: draft-capture — {} never stopped growing within {}s; the recording \
         may be truncated",
        results.display(),
        SETTLE_LIMIT.as_secs()
    );
}

/// The one trace a leg wrote into `results`.
///
/// Named by whichever recorder produced it — the host uses scenario + pid, the
/// tap uses its own sequence — so this reads the directory rather than
/// duplicating either convention, which is what keeps concurrent runs from
/// colliding.
fn sole_trace(results: &Path) -> Result<PathBuf, String> {
    let mut traces: Vec<PathBuf> = std::fs::read_dir(results)
        .map_err(|error| format!("cannot read {}: {error}", results.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    traces.sort();
    traces
        .pop()
        .ok_or_else(|| format!("nothing was recorded into {}", results.display()))
}
