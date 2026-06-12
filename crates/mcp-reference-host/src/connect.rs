// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Transport construction: the host's two ways onto a real wire.
//!
//! Both come from rmcp's official client features (ADR-0009: the host is an
//! rmcp client, not a protocol reimplementation): a child process speaking
//! stdio, and streamable HTTP over the reqwest-backed transport.

/// Splits a command line on spaces into program and arguments.
///
/// Deliberately the *same* convention the pinned suite's runner applies to
/// `--command` (ADR-0009 records the decoded contract), so a command that
/// works under the runner works here and vice versa. No quoting, no shell:
/// arguments containing spaces are not representable, exactly as under the
/// runner.
#[must_use]
pub fn split_command(command: &str) -> Option<(String, Vec<String>)> {
    let mut parts = command.split(' ').filter(|part| !part.is_empty());
    let program = parts.next()?.to_owned();
    Some((program, parts.map(ToOwned::to_owned).collect()))
}

/// A child-process stdio transport for `command` (split per
/// [`split_command`]). The child's stderr is inherited so its diagnostics
/// reach the operator; its stdout/stdin belong to the protocol.
///
/// # Errors
///
/// Returns an invalid-input error for an empty command, or the spawn error.
#[cfg(feature = "proc")]
pub fn child_process(command: &str) -> std::io::Result<rmcp::transport::TokioChildProcess> {
    let (program, args) = split_command(command).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty server command")
    })?;
    let mut spawn = tokio::process::Command::new(program);
    spawn.args(args);
    rmcp::transport::TokioChildProcess::new(spawn)
}

/// A streamable HTTP transport for `url` (the suite passes the scenario
/// server's URL as the command's final argument; general use passes
/// `--url`).
#[cfg(feature = "http")]
#[must_use]
pub fn streamable_http(
    url: &str,
) -> rmcp::transport::StreamableHttpClientTransport<reqwest::Client> {
    rmcp::transport::StreamableHttpClientTransport::from_uri(url)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn split_command_mirrors_the_runner_convention() {
        assert_eq!(
            split_command("server --transport stdio"),
            Some((
                "server".to_owned(),
                vec!["--transport".to_owned(), "stdio".to_owned()]
            ))
        );
        // Repeated spaces collapse (the runner's split produces empty
        // strings; spawning ignores empty argv entries only because we
        // filter them — pinned here).
        assert_eq!(
            split_command("server  --flag"),
            Some(("server".to_owned(), vec!["--flag".to_owned()]))
        );
        assert_eq!(split_command(""), None);
        assert_eq!(split_command("   "), None);
    }

    #[cfg(feature = "proc")]
    #[test]
    fn child_process_rejects_an_empty_command() {
        // (`TokioChildProcess` has no Debug impl, so no expect_err here.)
        let Err(error) = child_process("  ") else {
            panic!("empty command cannot spawn");
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
