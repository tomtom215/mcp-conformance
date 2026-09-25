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
//! and never alter traffic to fit the trace: what cannot be recorded (non-JSON output,
//! a message over the size limit) is forwarded intact and counted. Neither depends on
//! any MCP SDK, so the trace describes the bytes on the wire, not one SDK's reading of
//! them.

pub mod framing;
pub mod headers;
pub mod http;
pub mod recorder;
pub mod stdio;

pub use recorder::{NotRecorded, Recorder, Summary};

/// A future that resolves when this process is asked to stop: SIGINT or SIGTERM on
/// Unix, Ctrl-C elsewhere.
///
/// The Unix handlers are registered by this call, not when the future is first
/// polled, so a signal arriving between the call and the first poll is not lost —
/// the difference between a capture that finishes its trace and one the default
/// handler kills mid-announcement.
///
/// # Errors
///
/// A signal handler could not be registered.
pub fn shutdown_signal() -> std::io::Result<impl std::future::Future<Output = ()>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        Ok(async move {
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
            }
        })
    }
    #[cfg(not(unix))]
    {
        Ok(async {
            if tokio::signal::ctrl_c().await.is_err() {
                std::future::pending::<()>().await;
            }
        })
    }
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
