// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The suite's command lines, one per invocation, and running them.

use std::path::{Path, PathBuf};

use mcp_conformance::suite;
use mcp_trace_capture::Signals;
use tokio::process::Command;

use crate::node;

/// What every invocation shares.
#[derive(Debug, Clone)]
pub(crate) struct Common {
    pub(crate) work_dir: PathBuf,
    pub(crate) results: PathBuf,
    pub(crate) spec_version: String,
    pub(crate) expected_failures: Option<PathBuf>,
}

impl Common {
    fn command(&self, party: &str) -> Command {
        let mut command = node::suite(&self.work_dir);
        command.arg(party);
        command
    }

    fn finish(&self, command: &mut Command) {
        command.arg("--spec-version").arg(&self.spec_version);
        if let Some(path) = &self.expected_failures {
            command.arg("--expected-failures").arg(path);
        }
        command.arg("--output-dir").arg(&self.results);
    }

    /// The server scenarios at `url`: one invocation for the suite's active set,
    /// or one per named scenario (the suite takes one `--scenario` at a time).
    pub(crate) fn server(&self, url: &str, scenarios: &[String]) -> Vec<Command> {
        let one = |scenario: Option<&String>| {
            let mut command = self.command("server");
            command.arg("--url").arg(url);
            if let Some(scenario) = scenario {
                command.arg("--scenario").arg(scenario);
            }
            self.finish(&mut command);
            command
        };
        if scenarios.is_empty() {
            vec![one(None)]
        } else {
            scenarios
                .iter()
                .map(|scenario| one(Some(scenario)))
                .collect()
        }
    }

    /// One invocation per client scenario, sequentially: the suite's parallel
    /// mode would put the `sse-retry` timing under load.
    pub(crate) fn client(&self, client_command: &str, scenarios: &[String]) -> Vec<Command> {
        scenarios
            .iter()
            .map(|scenario| {
                let mut command = self.command("client");
                command
                    .arg("--scenario")
                    .arg(scenario)
                    .arg("--command")
                    .arg(client_command);
                self.finish(&mut command);
                command
            })
            .collect()
    }

    /// The suite's server scenarios for the spec version, as `list` prints them.
    ///
    /// # Errors
    ///
    /// The suite could not be run, or listed nothing.
    pub(crate) async fn server_scenarios(&self) -> Result<Vec<String>, String> {
        let mut command = self.command("list");
        command
            .arg("--server")
            .arg("--spec-version")
            .arg(&self.spec_version);
        let output = command
            .stderr(std::process::Stdio::inherit())
            .output()
            .await
            .map_err(|error| format!("cannot run npx: {error}"))?;
        let scenarios = listed(&String::from_utf8_lossy(&output.stdout));
        if !output.status.success() || scenarios.is_empty() {
            return Err(format!(
                "`{} list --server` listed no scenario for {}",
                suite::PACKAGE,
                self.spec_version
            ));
        }
        Ok(scenarios)
    }
}

/// The scenario names in the suite's `list` output (`  - name [versions]`).
pub(crate) fn listed(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("- "))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(ToOwned::to_owned)
        .collect()
}

/// The client command the suite is given: this binary's relay, then the
/// client's own command. The suite runs it through a shell, so the binary's
/// path is quoted when it needs to be.
pub(crate) fn relayed(exe: &Path, client_command: &str) -> String {
    format!(
        "{} __client-relay {client_command}",
        quoted(&exe.to_string_lossy())
    )
}

fn quoted(path: &str) -> String {
    let plain = |byte: u8| byte.is_ascii_alphanumeric() || b"/._-:\\".contains(&byte);
    if path.bytes().all(plain) {
        path.to_owned()
    } else if cfg!(windows) {
        format!("\"{path}\"")
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

/// Runs each invocation in turn. Returns whether every one passed, or `Err`
/// with the stop when this process is asked to stop (the invocation running
/// then is killed).
///
/// # Errors
///
/// An invocation could not be run.
pub(crate) async fn run_all(
    commands: Vec<Command>,
    signals: &mut Signals,
) -> Result<Result<bool, mcp_trace_capture::Stop>, String> {
    let mut passed = true;
    for command in commands {
        tokio::select! {
            ran = node::run(command) => passed &= ran?,
            stop = signals.recv() => return Ok(Err(stop)),
        }
    }
    Ok(Ok(passed))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn args(command: &Command) -> Vec<String> {
        command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn common(expected: Option<&str>) -> Common {
        Common {
            work_dir: PathBuf::from("/w"),
            results: PathBuf::from("/w/suite"),
            spec_version: "2025-11-25".to_owned(),
            expected_failures: expected.map(PathBuf::from),
        }
    }

    #[test]
    fn server_runs_are_one_for_the_active_set_or_one_per_scenario() {
        let pin = format!("{}@{}", suite::PACKAGE, suite::VERSION);
        let all = common(None).server("http://127.0.0.1:1/mcp", &[]);
        assert_eq!(all.len(), 1);
        assert_eq!(
            args(&all[0]),
            [
                "--yes",
                &pin,
                "server",
                "--url",
                "http://127.0.0.1:1/mcp",
                "--spec-version",
                "2025-11-25",
                "--output-dir",
                "/w/suite"
            ]
        );
        let named = common(Some("/b.yaml")).server(
            "http://x/mcp",
            &["ping".to_owned(), "tools-list".to_owned()],
        );
        assert_eq!(named.len(), 2);
        assert_eq!(
            args(&named[1])[2..],
            [
                "server",
                "--url",
                "http://x/mcp",
                "--scenario",
                "tools-list",
                "--spec-version",
                "2025-11-25",
                "--expected-failures",
                "/b.yaml",
                "--output-dir",
                "/w/suite"
            ]
        );
    }

    #[test]
    fn client_runs_are_one_per_scenario() {
        let runs = common(None).client(
            "relay client",
            &["initialize".to_owned(), "sse-retry".to_owned()],
        );
        assert_eq!(runs.len(), 2);
        assert_eq!(
            args(&runs[1])[2..],
            [
                "client",
                "--scenario",
                "sse-retry",
                "--command",
                "relay client",
                "--spec-version",
                "2025-11-25",
                "--output-dir",
                "/w/suite"
            ]
        );
    }

    #[test]
    fn the_list_is_read_by_its_bullets() {
        let output = "Server scenarios (test against a server):\n  - server-initialize [2025-06-18,2025-11-25]\n  - ping [2025-11-25]\n\nnot a bullet\n";
        assert_eq!(listed(output), ["server-initialize", "ping"]);
        assert_eq!(listed("").len(), 0);
    }

    #[test]
    fn the_relay_command_quotes_this_binary_only_when_needed() {
        assert_eq!(
            relayed(Path::new("/usr/bin/mcp-conformance"), "node client.js --x"),
            "/usr/bin/mcp-conformance __client-relay node client.js --x"
        );
        if cfg!(unix) {
            assert_eq!(
                relayed(Path::new("/my dir/it's"), "c"),
                r"'/my dir/it'\''s' __client-relay c"
            );
        }
    }
}
