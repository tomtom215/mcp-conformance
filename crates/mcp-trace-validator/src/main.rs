// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-trace-validator` — validate recorded MCP protocol traces offline.
//!
//! The command line itself is [`mcp_trace_validator::cli`], which
//! `mcp-conformance` shares. Exit codes (stable interface, relied on by CI
//! integrations):
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | Validation ran; no MUST-level violations (warnings allowed unless `--strict`) |
//! | 1    | MUST-level violations — or SHOULD-level ones under `--strict`, which says so on stderr |
//! | 2    | Invocation, registry, or check-inventory problem (including `unsupported` outcomes), a trace that judges nothing, or output not written |
//! | 3    | The trace document itself was malformed |

use std::process::ExitCode;

use clap::{CommandFactory as _, FromArgMatches as _, Parser, Subcommand};
use mcp_trace_validator::cli;

/// Offline conformance validation for recorded Model Context Protocol traces.
#[derive(Debug, Parser)]
#[command(name = "mcp-trace-validator", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a JSON Lines trace against the specification.
    ///
    /// The protocol revision is the one the trace declares (in `initialize`, per-request
    /// `_meta`, or the `MCP-Protocol-Version` header); a trace declaring none is judged
    /// against the newest supported revision. `--revision` overrides the choice; naming
    /// several judges the trace under each, clause by clause.
    #[command(
        after_help = "Exit status: 0 pass (warnings pass unless --strict), 1 a clause \
        failed, 2 bad invocation or nothing judgeable or output not written, 3 malformed trace."
    )]
    Validate(cli::ValidateArgs),
    /// Print the requirement registry this build validates against, part of
    /// it, or single requirements in full.
    Requirements(cli::RequirementsArgs),
    /// Print a shell completion script.
    ///
    /// Bash: `mcp-trace-validator completions bash > ~/.local/share/bash-completion/completions/mcp-trace-validator`.
    /// Zsh: write `_mcp-trace-validator` to a directory on `$fpath`. Fish:
    /// `~/.config/fish/completions/mcp-trace-validator.fish`.
    Completions {
        /// The shell to complete for.
        shell: clap_complete::Shell,
    },
}

fn main() -> ExitCode {
    let matches = Cli::command()
        .long_version(cli::long_version(env!("CARGO_PKG_VERSION")))
        .get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    let code = match cli.command {
        Command::Validate(args) => cli::validate(&args),
        Command::Requirements(args) => cli::requirements(&args),
        Command::Completions { shell } => {
            cli::completions(shell, &mut Cli::command(), "mcp-trace-validator")
        }
    };
    ExitCode::from(code)
}
