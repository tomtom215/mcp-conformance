// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};

use super::{Unrecorded, pump};
use crate::recorder::Recorder;

/// Runs a test body with a deadline, so a regression that stops data flowing fails
/// the test instead of hanging it (mutation testing counts a hang as a timeout,
/// which fails CI without saying why).
async fn bounded(body: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(std::time::Duration::from_secs(20), body)
        .await
        .expect("the test finished within 20 s");
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn events(sink: &Shared) -> Vec<TraceEvent> {
    String::from_utf8(sink.0.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn bytes_pass_through_unchanged_and_unrecordable_lines_are_counted() {
    bounded(async {
        let sink = Shared::default();
        let recorder = Recorder::new(sink.clone());
        let input: &[u8] =
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\nnot json\n\n{\"partial\":tr";
        let (mut writer, reader) = tokio::io::duplex(64);
        let (forward, mut forwarded) = tokio::io::duplex(1024);
        let feeder = tokio::spawn(async move {
            // Deliberately small writes, so lines cross chunk boundaries.
            for piece in input.chunks(5) {
                writer.write_all(piece).await.unwrap();
            }
        });
        let unrecorded = pump(&recorder, Direction::ClientToServer, reader, forward, 1024)
            .await
            .unwrap();
        feeder.await.unwrap();
        let mut out = Vec::new();
        forwarded.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, input, "the peer receives exactly what was sent");
        // The unterminated final line is recorded only if it is JSON; this one is not.
        assert_eq!(
            unrecorded,
            Unrecorded {
                not_json: 3,
                oversized: 0
            }
        );
        let trace = events(&sink);
        assert_eq!(trace.len(), 1);
        assert!(
            matches!(&trace[0].body, EventBody::Message { payload } if payload["method"] == "ping")
        );
    })
    .await;
}

/// The ordering guarantee: a peer that answers the instant a request arrives still
/// never gets its answer recorded first, over many round trips.
// One scenario end to end: four pipes, three tasks and the assertion over the
// whole trace read better together than split into helpers.
#[allow(clippy::too_many_lines)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_response_is_never_recorded_before_its_request() {
    bounded(async {
        let sink = Shared::default();
        let recorder = Arc::new(Recorder::new(sink.clone()));
        let (mut client_out, client_side) = tokio::io::duplex(1 << 16);
        let (server_in, server_side_in) = tokio::io::duplex(1 << 16);
        let (server_side_out, server_out) = tokio::io::duplex(1 << 16);
        let (client_in, mut client_reads) = tokio::io::duplex(1 << 16);

        // The "server": echo each request line back as a response with the same id.
        let server = tokio::spawn(async move {
            let mut lines = BufReader::new(server_side_in).lines();
            let mut out = server_side_out;
            while let Some(line) = lines.next_line().await.unwrap() {
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let response =
                    serde_json::json!({"jsonrpc": "2.0", "id": request["id"], "result": {}});
                out.write_all(format!("{response}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let up = tokio::spawn({
            let recorder = Arc::clone(&recorder);
            async move {
                pump(
                    &recorder,
                    Direction::ClientToServer,
                    client_side,
                    server_in,
                    1 << 16,
                )
                .await
            }
        });
        let down = tokio::spawn({
            let recorder = Arc::clone(&recorder);
            async move {
                pump(
                    &recorder,
                    Direction::ServerToClient,
                    server_out,
                    client_in,
                    1 << 16,
                )
                .await
            }
        });
        let drain = tokio::spawn(async move {
            let mut sink = Vec::new();
            client_reads.read_to_end(&mut sink).await.unwrap();
        });
        for id in 0..500 {
            client_out
                .write_all(
                    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"ping\"}}\n").as_bytes(),
                )
                .await
                .unwrap();
        }
        drop(client_out);
        up.await.unwrap().unwrap();
        server.await.unwrap();
        down.await.unwrap().unwrap();
        drain.await.unwrap();

        let trace = events(&sink);
        assert_eq!(trace.len(), 1000);
        let mut request_seq = std::collections::HashMap::new();
        for event in &trace {
            let EventBody::Message { payload } = &event.body else {
                panic!("only messages are recorded here");
            };
            let id = payload["id"].as_u64().unwrap();
            match event.direction {
                Direction::ClientToServer => {
                    request_seq.insert(id, event.seq);
                }
                Direction::ServerToClient => {
                    let request = request_seq
                        .get(&id)
                        .unwrap_or_else(|| panic!("response {id} recorded before its request"));
                    assert!(*request < event.seq);
                }
            }
        }
    })
    .await;
}
