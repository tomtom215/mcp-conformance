// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `test`: the pinned suite against the subject, through the recorder; every
//! recorded session judged; both verdicts reported, and whether they agree.
//!
//! The steps, each a reason to stop with exit 2 when it cannot be taken:
//! Node and npx present; the baselines readable; a fresh work directory; the
//! suite fetched and reporting the pinned version; the recorder in front of the
//! subject (the HTTP proxy for `--url` and for a client, the stdio bridge for
//! `--command`); the suite run; its results read back; the sessions judged.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use mcp_conformance_cli::agreement::{self, Baseline, Reconciliation};
use mcp_conformance_cli::suite::{self, Scenario};
use mcp_trace_capture::Signals;
use mcp_trace_validator::report::Verdict;

use crate::args::{Subject, TestArgs};
use crate::judging::{self, Judging};
use crate::{EXIT_FINDINGS, EXIT_OK, EXIT_USAGE, bridge, node, proxy, relay, render};

mod invoke;

use invoke::Common;

/// What a run found, for the report.
#[derive(Debug)]
pub(crate) struct Outcome {
    /// The subject as the report names it (a URL without its query).
    pub(crate) subject: String,
    pub(crate) spec_version: String,
    /// Whether every suite invocation exited successfully.
    pub(crate) suite_passed: bool,
    pub(crate) scenarios: Vec<Scenario>,
    /// Scenarios deliberately not run, with why.
    pub(crate) not_run: Vec<(String, &'static str)>,
    pub(crate) judging: Judging,
    pub(crate) reconciliation: Reconciliation,
    /// The divergence baseline's path, or `None` without one.
    pub(crate) baseline: Option<PathBuf>,
    pub(crate) work_dir: PathBuf,
}

impl Outcome {
    /// The exit code: 0 when the suite passed and the two agree.
    pub(crate) fn exit_code(&self) -> u8 {
        if self.judging.verdict() == Verdict::Unsupported {
            // A registry this build cannot judge with: a build problem, as the
            // validator reports it.
            EXIT_USAGE
        } else if self.suite_passed && self.reconciliation.holds() {
            EXIT_OK
        } else {
            EXIT_FINDINGS
        }
    }
}

/// Runs `test`; returns the exit code.
pub(crate) async fn test(args: TestArgs) -> u8 {
    let report = args.report.clone();
    match outcome(args).await {
        Ok(outcome) => {
            if !render::write(&outcome, &report) {
                return EXIT_USAGE;
            }
            render::summary(&outcome);
            outcome.exit_code()
        }
        Err(message) => {
            eprintln!("mcp-conformance: {message}");
            EXIT_USAGE
        }
    }
}

async fn outcome(args: TestArgs) -> Result<Outcome, String> {
    node::check().await?;
    let (baseline, expected_failures) = inputs(&args)?;
    let work_dir = work_dir(args.work_dir.as_deref())?;
    eprintln!("mcp-conformance: work directory {}", work_dir.display());
    let traces = work_dir.join("traces");
    let results = work_dir.join("suite");
    for dir in [&traces, &results] {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    }
    node::verify_suite(&work_dir).await?;
    let common = Common {
        work_dir: work_dir.clone(),
        results: results.clone(),
        spec_version: args.spec_version.clone(),
        expected_failures,
    };
    let mut signals =
        Signals::install().map_err(|error| format!("cannot install a signal handler: {error}"))?;
    let (subject, suite_passed, not_run) = drive(
        &args.subject,
        &args.scenarios,
        &common,
        &traces,
        &mut signals,
    )
    .await?;
    let scenarios = suite::read_results(&results)?;
    if scenarios.is_empty() {
        return Err(format!(
            "the suite wrote no results under {} — it did not run (npm registry unreachable? \
             a broken npx cache?); this is not a conformance verdict",
            results.display()
        ));
    }
    let judging = judging::judge_dir(&traces)?;
    if judging.judged.is_empty() {
        return Err(format!(
            "no recorded session under {} could be judged ({} recorded); the recording \
             failed, or the subject never answered",
            traces.display(),
            judging.unjudged.len()
        ));
    }
    let reconciliation = agreement::reconcile(&baseline, &judging.failures());
    Ok(Outcome {
        subject,
        spec_version: args.spec_version,
        suite_passed,
        scenarios,
        not_run,
        judging,
        reconciliation,
        baseline: args.divergences,
        work_dir,
    })
}

/// The divergence baseline (empty without one) and the suite's
/// expected-failures file, made absolute: npx runs in the work directory.
fn inputs(args: &TestArgs) -> Result<(Baseline, Option<PathBuf>), String> {
    let baseline = match &args.divergences {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            Baseline::parse(&text).map_err(|error| format!("{}: {error}", path.display()))?
        }
        None => Baseline::default(),
    };
    let expected_failures = args
        .expected_failures
        .as_deref()
        .map(|path| {
            std::path::absolute(path)
                .ok()
                .filter(|path| path.is_file())
                .ok_or_else(|| format!("--expected-failures: {} is not a file", path.display()))
        })
        .transpose()?;
    Ok((baseline, expected_failures))
}

type Driven = (String, bool, Vec<(String, &'static str)>);

/// Puts the recorder in front of the subject and runs the suite through it.
async fn drive(
    subject: &Subject,
    scenarios: &[String],
    common: &Common,
    traces: &Path,
    signals: &mut Signals,
) -> Result<Driven, String> {
    if let Some(url) = &subject.url {
        let (upstream, path) = proxy::split(url).map_err(|error| format!("--url: {error}"))?;
        let preserve_host = proxy::is_loopback(&upstream);
        if !preserve_host {
            eprintln!(
                "mcp-conformance: the recording proxy sends the server's own name as `Host` \
                 (it is not on this machine), so the suite's dns-rebinding-protection \
                 scenario sees the proxy's Host, not the one it sent"
            );
        }
        let shown = shown(&upstream, &path);
        let proxy = proxy::start(upstream, preserve_host, traces).await?;
        let url = format!("http://{}{path}", proxy.address);
        let ran = invoke::run_all(common.server(&url, scenarios), signals).await;
        let stopped = proxy.stop().await;
        let passed = finished(ran)?;
        stopped?;
        Ok((shown, passed, Vec::new()))
    } else if let Some(command) = &subject.command {
        let bridge = bridge::start(command, traces).await?;
        let (scenarios, not_run) = if scenarios.is_empty() {
            stdio_scenarios(common.server_scenarios().await?)
        } else {
            (scenarios.to_vec(), Vec::new())
        };
        let url = format!("http://{}/mcp", bridge.address);
        let ran = invoke::run_all(common.server(&url, &scenarios), signals).await;
        bridge.stop().await;
        Ok((command.clone(), finished(ran)?, not_run))
    } else if let Some(client) = &subject.client_command {
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot find this program's own path: {error}"))?;
        let scenarios: Vec<String> = if scenarios.is_empty() {
            suite::CLIENT_SCENARIOS
                .iter()
                .map(|s| (*s).to_owned())
                .collect()
        } else {
            scenarios.to_vec()
        };
        let mut commands = common.client(&invoke::relayed(&exe, client), &scenarios);
        for command in &mut commands {
            command.env(relay::RELAY_DIR, traces);
        }
        let ran = invoke::run_all(commands, signals).await;
        Ok((client.clone(), finished(ran)?, Vec::new()))
    } else {
        Err("name the subject: --url, --command or --client-command".to_owned())
    }
}

/// The suite's verdict, or why there is none.
fn finished(ran: Result<Result<bool, mcp_trace_capture::Stop>, String>) -> Result<bool, String> {
    match ran? {
        Ok(passed) => Ok(passed),
        Err(stop) => Err(format!(
            "{}: stopped before the suite finished",
            stop.name()
        )),
    }
}

/// The suite's listed scenarios a stdio server is run against: its default
/// (`active`) set, less the HTTP-transport scenarios, which are reported as
/// not run with why.
fn stdio_scenarios(listed: Vec<String>) -> (Vec<String>, Vec<(String, &'static str)>) {
    let mut run = Vec::new();
    let mut not_run = Vec::new();
    for scenario in listed {
        if suite::HTTP_TRANSPORT_SCENARIOS.contains(&scenario.as_str()) {
            not_run.push((
                scenario,
                "exercises the streamable-HTTP transport, which a stdio server does not have",
            ));
        } else if suite::PENDING_SERVER_SCENARIOS.contains(&scenario.as_str()) {
            not_run.push((
                scenario,
                "pending in the pinned suite: its default run leaves it out",
            ));
        } else {
            run.push(scenario);
        }
    }
    (run, not_run)
}

/// A URL as reports show it: without its query, which can carry a key.
fn shown(upstream: &axum::http::Uri, path: &str) -> String {
    let origin = upstream.to_string();
    let origin = origin.trim_end_matches('/');
    match path.split_once('?') {
        Some((path, _)) => format!("{origin}{path}?…"),
        None => format!("{origin}{path}"),
    }
}

/// The work directory: `requested` if it is new or empty, else a new one
/// under the system's temporary directory. Returned absolute: npx runs in it.
fn work_dir(requested: Option<&Path>) -> Result<PathBuf, String> {
    let dir = if let Some(dir) = requested {
        if std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some()) {
            return Err(format!(
                "--work-dir {} is not empty; name a new or empty directory",
                dir.display()
            ));
        }
        dir.to_path_buf()
    } else {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        std::env::temp_dir().join(format!("mcp-conformance-{stamp}-{}", std::process::id()))
    };
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    std::path::absolute(&dir).map_err(|error| format!("{}: {error}", dir.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests;
