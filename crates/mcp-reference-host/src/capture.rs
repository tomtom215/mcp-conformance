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
//!
//! **An outbound message is recorded as it is handed to the inner transport,
//! not when that transport reports the send complete.** [`Transport::send`]
//! returns a `'static` future the caller may hold, spawn, or poll late, so
//! rmcp can receive and hand us a *reply* while the send future for the
//! request is still pending. Recording at completion therefore wrote the
//! response ahead of its request, and `seq` is the trace's only ordering
//! authority: every correlation check — response-id matching, cancellation
//! windows, cursor provenance — reads a reply-before-request pair as a
//! protocol violation by the server. It cost this workspace a phantom
//! `BASE-046` failure on one capture in three before it was found.
//!
//! The cost of the fix is stated rather than hidden: a message whose send
//! then *fails* is in the trace although it never reached the wire. That is a
//! strictly smaller error — it happens only when the transport is dying, and
//! it misrepresents one message instead of reordering every concurrent
//! exchange — and the failure is reported on stderr where an operator sees
//! it.

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
        // transport), and record here — synchronously, before the item is
        // handed over — so the request is ordered ahead of any reply it
        // draws. See this module's header for why completion is too late.
        match serde_json::to_value(&item) {
            Ok(payload) => self
                .recorder
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .record("client-to-server", &payload),
            Err(_) => eprintln!(
                "mcp-reference-host: trace capture skipped an unserializable outbound message"
            ),
        }
        let sending = self.inner.send(item);
        async move {
            if let Err(error) = sending.await {
                // The line is already in the trace; say so, rather than
                // leaving an operator to wonder why a recorded request was
                // never answered.
                eprintln!(
                    "mcp-reference-host: an outbound message was recorded but its send failed"
                );
                return Err(error);
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

    /// A transport whose `send` future only completes when told to, standing
    /// in for the real thing: rmcp holds these futures and may poll them late,
    /// which is precisely the window a reply can arrive in.
    struct DeferredSend {
        gate: Arc<tokio::sync::Notify>,
        queue: std::collections::VecDeque<RxJsonRpcMessage<RoleClient>>,
    }

    impl Transport<RoleClient> for DeferredSend {
        type Error = std::io::Error;
        fn send(
            &mut self,
            item: TxJsonRpcMessage<RoleClient>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            let value = serde_json::to_value(&item).unwrap();
            if let Some(id) = value.get("id").cloned() {
                self.queue.push_back(
                    serde_json::from_value(serde_json::json!({
                        "jsonrpc": "2.0", "id": id, "result": {"late": true},
                    }))
                    .unwrap(),
                );
            }
            let gate = Arc::clone(&self.gate);
            async move {
                gate.notified().await;
                Ok(())
            }
        }
        async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
            self.queue.pop_front()
        }
        async fn close(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_request_is_recorded_before_the_reply_it_draws() {
        // The regression this pins cost a phantom BASE-046 failure on roughly
        // one capture in three: `seq` was assigned when the *send future*
        // completed, so a reply received while that future was still pending
        // landed in the trace ahead of its own request — which every
        // correlation check reads as the server answering an id nobody asked.
        let dir = std::env::temp_dir().join(format!("host-order-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let gate = Arc::new(tokio::sync::Notify::new());
        let inner = DeferredSend {
            gate: Arc::clone(&gate),
            queue: std::collections::VecDeque::new(),
        };
        let mut transport =
            RecordingTransport::create(inner, CaptureTransport::Stdio, &path).unwrap();

        let request: TxJsonRpcMessage<RoleClient> =
            serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":9,"method":"ping"}))
                .unwrap();
        // Hold the send future unfinished, exactly as a busy transport does…
        let sending = transport.send(request);
        // …and take delivery of the reply while it is still pending.
        let received = transport.receive().await.expect("the reply arrives first");
        assert_eq!(
            serde_json::to_value(&received).unwrap()["result"]["late"],
            true
        );
        gate.notify_one();
        sending.await.unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(
            lines[0]["payload"]["method"], "ping",
            "the request is first"
        );
        assert_eq!(lines[0]["seq"], 0);
        assert_eq!(lines[1]["payload"]["result"]["late"], true);
        assert_eq!(lines[1]["seq"], 1);
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
