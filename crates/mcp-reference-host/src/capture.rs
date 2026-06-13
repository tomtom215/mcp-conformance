// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Host-side trace capture: a [`Transport`] wrapper recording every JSON-RPC
//! message the host sends or receives as a validator-ready JSON Lines trace.
//!
//! **Redaction by construction** (05-security-model.md): the [`Transport`]
//! seam carries only protocol messages — HTTP headers, URLs, and any
//! credential material live below it and never reach this recorder, so a
//! host trace cannot leak them. The cost of that guarantee is scope: traces
//! carry no `kind: http` events, so the validator's header-level transport
//! checks (session-id echo, `Accept`, content-type) report nothing rather
//! than judging unobserved headers — exactly the not-applicable-over-vacuous
//! posture 03-conformance-strategy.md requires.
//!
//! Write discipline matches the everything-server's tap: one line per event,
//! written and flushed before the call returns, `seq` assigned in record
//! order under the writer lock so the schema's strictly-increasing rule
//! holds by construction. A recording failure is reported to stderr and the
//! exchange continues unrecorded — capture is diagnostics, never the thing
//! that takes the host down.

use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use rmcp::service::{RoleClient, RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;

/// Which wire the recorded session ran over, in the trace schema's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTransport {
    /// A child process speaking newline-delimited JSON-RPC (`stdio`).
    Stdio,
    /// Streamable HTTP (`streamable-http`).
    StreamableHttp,
}

impl CaptureTransport {
    /// The trace schema's `transport` field value.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::StreamableHttp => "streamable-http",
        }
    }
}

/// Shared recorder state: the open trace file and the next `seq`.
struct Recorder {
    file: std::io::LineWriter<std::fs::File>,
    next_seq: u64,
    transport: CaptureTransport,
}

impl Recorder {
    /// Appends one message event; `seq` is assigned here, under the lock.
    fn record(&mut self, direction: &str, payload: &serde_json::Value) {
        let event = serde_json::json!({
            "seq": self.next_seq,
            "direction": direction,
            "transport": self.transport.as_str(),
            "kind": "message",
            "payload": payload,
        });
        // LineWriter flushes on the newline, so every completed call leaves a
        // durable line — the same per-record durability the server tap gives.
        let written = serde_json::to_string(&event)
            .map_err(std::io::Error::other)
            .and_then(|line| writeln!(self.file, "{line}"));
        match written {
            Ok(()) => self.next_seq += 1,
            Err(error) => {
                eprintln!("mcp-reference-host: trace capture write failed: {error}");
            }
        }
    }
}

/// A [`Transport`] wrapper that records traffic to a JSON Lines trace file.
///
/// Sent messages are recorded only after the inner transport accepts them
/// (an event must describe wire truth, not intent); received messages are
/// recorded before they are handed to the service.
pub struct RecordingTransport<T> {
    inner: T,
    recorder: Arc<Mutex<Recorder>>,
}

impl<T> std::fmt::Debug for RecordingTransport<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingTransport").finish_non_exhaustive()
    }
}

impl<T> RecordingTransport<T> {
    /// Wraps `inner`, recording to `path` (created or truncated).
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the trace file cannot be created.
    pub fn create(inner: T, transport: CaptureTransport, path: &Path) -> std::io::Result<Self> {
        let file = std::fs::File::create(path)?;
        Ok(Self {
            inner,
            recorder: Arc::new(Mutex::new(Recorder {
                file: std::io::LineWriter::new(file),
                next_seq: 0,
                transport,
            })),
        })
    }
}

impl<T: Transport<RoleClient> + Send> Transport<RoleClient> for RecordingTransport<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        // Serialize before sending (the value is moved into the inner
        // transport); record only once the inner send succeeded.
        let payload = serde_json::to_value(&item).ok();
        let recorder = Arc::clone(&self.recorder);
        let sending = self.inner.send(item);
        async move {
            sending.await?;
            match payload {
                Some(payload) => recorder
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .record("client-to-server", &payload),
                None => eprintln!(
                    "mcp-reference-host: trace capture skipped an unserializable outbound message"
                ),
            }
            Ok(())
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        let item = self.inner.receive().await?;
        match serde_json::to_value(&item) {
            Ok(payload) => self
                .recorder
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .record("server-to-client", &payload),
            Err(_) => eprintln!(
                "mcp-reference-host: trace capture skipped an unserializable inbound message"
            ),
        }
        Some(item)
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.inner.close().await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A loopback transport: everything sent is echoed back as a received
    /// "result" so both record directions run without any real wire. The
    /// close flag proves delegation — a recorder that swallows `close`
    /// would leak the inner transport's resources.
    struct EchoTransport {
        queue: std::collections::VecDeque<RxJsonRpcMessage<RoleClient>>,
        closed: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Transport<RoleClient> for EchoTransport {
        type Error = std::io::Error;
        fn send(
            &mut self,
            item: TxJsonRpcMessage<RoleClient>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            // Echo a minimal result for requests; swallow notifications.
            let value = serde_json::to_value(&item).unwrap();
            if let Some(id) = value.get("id").cloned() {
                let reply = serde_json::from_value(serde_json::json!({
                    "jsonrpc": "2.0", "id": id, "result": {"echoed": true},
                }))
                .unwrap();
                self.queue.push_back(reply);
            }
            std::future::ready(Ok(()))
        }
        async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
            self.queue.pop_front()
        }
        async fn close(&mut self) -> Result<(), Self::Error> {
            self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn records_both_directions_with_contiguous_seq() {
        let dir = std::env::temp_dir().join(format!("host-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let inner = EchoTransport {
            queue: std::collections::VecDeque::new(),
            closed: Arc::clone(&closed),
        };
        let mut transport =
            RecordingTransport::create(inner, CaptureTransport::Stdio, &path).unwrap();
        assert!(
            format!("{transport:?}").contains("RecordingTransport"),
            "Debug names the wrapper"
        );

        let ping: TxJsonRpcMessage<RoleClient> =
            serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":7,"method":"ping"}))
                .unwrap();
        transport.send(ping).await.unwrap();
        let received = transport.receive().await.expect("echo comes back");
        let received = serde_json::to_value(&received).unwrap();
        assert_eq!(received["result"]["echoed"], true);
        transport.close().await.unwrap();
        assert!(
            closed.load(std::sync::atomic::Ordering::SeqCst),
            "close must delegate to the inner transport"
        );

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "one sent + one received: {text}");
        assert_eq!(lines[0]["seq"], 0);
        assert_eq!(lines[0]["direction"], "client-to-server");
        assert_eq!(lines[0]["transport"], "stdio");
        assert_eq!(lines[0]["payload"]["method"], "ping");
        assert_eq!(lines[1]["seq"], 1);
        assert_eq!(lines[1]["direction"], "server-to-client");
        assert_eq!(lines[1]["payload"]["result"]["echoed"], true);

        // The pin that matters: the validator's real reader accepts the
        // capture's bytes verbatim — field names, transport vocabulary, seq
        // discipline. A schema drift in this module must fail here, not at
        // agreement time.
        let events = mcp_trace_validator::reader::parse_trace(
            &text,
            &mcp_trace_validator::reader::Limits::default(),
        )
        .expect("captured trace parses through the validator's reader");
        assert_eq!(events.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn both_transport_names_parse_through_the_real_reader() {
        // The schema's TransportKind serde names: pinned by parsing one
        // event of each kind through the validator's reader, so a typo in
        // `as_str` cannot survive (it would make every captured trace
        // unreadable downstream).
        for kind in [CaptureTransport::Stdio, CaptureTransport::StreamableHttp] {
            let line = serde_json::json!({
                "seq": 0,
                "direction": "client-to-server",
                "transport": kind.as_str(),
                "kind": "message",
                "payload": {"jsonrpc": "2.0", "id": 1, "method": "ping"},
            })
            .to_string();
            mcp_trace_validator::reader::parse_trace(
                &line,
                &mcp_trace_validator::reader::Limits::default(),
            )
            .unwrap_or_else(|error| panic!("{} must be schema-legal: {error}", kind.as_str()));
        }
    }
}
