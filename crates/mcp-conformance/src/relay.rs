// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `__client-relay`: a client under test, reaching the suite's server through
//! the recorder.
//!
//! For a client scenario the suite starts a server and runs the client command
//! with that server's URL appended. `test --client-command` hands the suite this
//! binary as the client command instead, followed by the real one; so the suite
//! runs `mcp-conformance __client-relay <client…> <url>`, and this:
//!
//! 1. starts the recorder's HTTP proxy in front of `<url>`'s origin, recording
//!    each session to the next `NNN.jsonl` under the run's trace directory
//!    (named by [`RELAY_DIR`]), in a directory per scenario;
//! 2. runs `<client…>` with the same URL on the proxy appended — same path, so
//!    the client asks for what it would have;
//! 3. when the client exits (or the suite stops this process), stops the proxy
//!    and exits with the client's code, which the suite reads as the client's.

use std::path::{Path, PathBuf};

use mcp_trace_capture::Signals;

use crate::{EXIT_USAGE, proxy};

/// The environment variable naming the run's trace directory. The suite
/// passes its environment on to the client command, so `test` sets it once.
pub(crate) const RELAY_DIR: &str = "MCP_CONFORMANCE_RELAY_DIR";

/// The variable the suite sets to the scenario it is running.
const SCENARIO: &str = "MCP_CONFORMANCE_SCENARIO";

/// The relay. Returns the client's exit code (low eight bits), `128 + signal`
/// when the suite stopped it, or 2 when it could not run.
pub(crate) async fn run(command: Vec<String>) -> u8 {
    match relay(command).await {
        Ok(code) => code,
        Err(message) => {
            eprintln!("mcp-conformance __client-relay: {message}");
            EXIT_USAGE
        }
    }
}

async fn relay(command: Vec<String>) -> Result<u8, String> {
    let (url, client) = command
        .split_last()
        .filter(|(_, client)| !client.is_empty())
        .ok_or_else(|| "expected the client command, then the server URL".to_owned())?;
    let root = std::env::var_os(RELAY_DIR)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{RELAY_DIR} is not set; this command is run by `test`"))?;
    let scenario = std::env::var(SCENARIO).unwrap_or_else(|_| "client".to_owned());
    let dir = scenario_dir(&root, &scenario);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let (upstream, path) = proxy::split(url).map_err(|error| format!("the server URL: {error}"))?;
    let mut signals =
        Signals::install().map_err(|error| format!("cannot install a signal handler: {error}"))?;
    let proxy = proxy::start(upstream, false, &dir).await?;
    let client_url = format!("http://{}{path}", proxy.address);
    let code = match tokio::process::Command::new(&client[0])
        .args(&client[1..])
        .arg(&client_url)
        .kill_on_drop(true)
        .spawn()
    {
        Ok(mut child) => tokio::select! {
            status = child.wait() => status.map_or(1, exit_code),
            stop = signals.recv() => {
                let _ = child.kill().await;
                u8::try_from(128 + stop.number()).unwrap_or(1)
            }
        },
        Err(error) => {
            eprintln!(
                "mcp-conformance __client-relay: cannot start the client {:?}: {error}",
                client[0]
            );
            EXIT_USAGE
        }
    };
    if let Err(message) = proxy.stop().await {
        eprintln!("mcp-conformance __client-relay: {message}");
        return Ok(if code == 0 { EXIT_USAGE } else { code });
    }
    Ok(code)
}

/// The scenario's trace directory: `auth/metadata-default` becomes
/// `auth-metadata-default`, so every scenario is one directory deep.
pub(crate) fn scenario_dir(root: &Path, scenario: &str) -> PathBuf {
    root.join(scenario.replace(['/', '\\'], "-"))
}

/// A client's exit status as one byte: its code, else (killed by a signal) 1.
fn exit_code(status: std::process::ExitStatus) -> u8 {
    status
        .code()
        .map_or(1, |code| u8::try_from(code & 0xFF).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_scenario_is_one_directory_deep() {
        let root = Path::new("/run");
        assert_eq!(scenario_dir(root, "initialize"), root.join("initialize"));
        assert_eq!(
            scenario_dir(root, "auth/metadata-default"),
            root.join("auth-metadata-default")
        );
        assert_eq!(scenario_dir(root, "a\\b"), root.join("a-b"));
    }

    #[tokio::test]
    async fn without_a_client_or_a_directory_it_refuses() {
        assert_eq!(
            run(vec!["http://localhost:1/".to_owned()]).await,
            EXIT_USAGE
        );
        assert_eq!(run(Vec::new()).await, EXIT_USAGE);
    }

    #[cfg(unix)]
    #[test]
    fn a_clients_code_is_kept() {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(exit_code(std::process::ExitStatus::from_raw(3 << 8)), 3);
        assert_eq!(exit_code(std::process::ExitStatus::from_raw(0)), 0);
        // Killed by SIGKILL: no code.
        assert_eq!(exit_code(std::process::ExitStatus::from_raw(9)), 1);
    }
}
