// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Record any Model Context Protocol session as a trace `mcp-trace-validator` can
//! judge.
//!
//! Two recorders, both transparent to the session they record:
//!
//! - [`stdio`] runs the server as a child process and sits on its stdin/stdout.
//!   A client configured to launch `mcp-trace-capture stdio -- <server>` in place of
//!   `<server>` works exactly as before.
//! - [`http`] is a reverse proxy in front of a streamable-HTTP server, SSE included.
//!
//! Both write the same JSON Lines trace ([`mcp_conformance_core::trace`]), record a
//! message before forwarding the bytes that complete it — so `seq` order is causal —
//! and never alter traffic to fit the trace: what cannot be recorded (a message over
//! the size limit) is forwarded intact and counted. A stdio line that is not JSON is
//! recorded as a string payload, for the validator to judge. Neither depends on
//! any MCP SDK, so the trace describes the bytes on the wire, not one SDK's reading of
//! them.

pub mod framing;
pub mod headers;
pub mod http;
pub mod numbered;
pub mod recorder;
pub mod signals;
pub mod stdio;
pub mod traces;

pub use recorder::{NotRecorded, Recorder, Summary};
pub use signals::{STOP_GRACE, Signals, Stop};

/// A future that resolves at the first request to stop: SIGINT, SIGTERM or SIGHUP
/// on Unix, Ctrl-C elsewhere.
///
/// The handlers are registered by this call, not when the future is first polled,
/// so a signal arriving between the call and the first poll is not lost. They stay
/// registered after the future resolves, so later signals are caught rather than
/// fatal; use [`Signals`] directly to see them.
///
/// # Errors
///
/// A signal handler could not be registered.
pub fn shutdown_signal() -> std::io::Result<impl std::future::Future<Output = ()>> {
    let mut signals = Signals::install()?;
    Ok(async move {
        signals.recv().await;
    })
}

/// The default largest message recorded, in bytes (64 MiB).
///
/// The toolkit-wide
/// [`DEFAULT_MAX_MESSAGE_BYTES`](mcp_conformance_core::trace::DEFAULT_MAX_MESSAGE_BYTES),
/// which the validator's default line limit is sized to read back.
pub const DEFAULT_MAX_MESSAGE: usize = mcp_conformance_core::trace::DEFAULT_MAX_MESSAGE_BYTES;

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_message_limit_is_the_documented_64_mib() {
        // The README and `--help` both state it; this keeps them honest.
        assert_eq!(super::DEFAULT_MAX_MESSAGE, 67_108_864);
    }
}
