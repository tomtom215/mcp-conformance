// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A stdio server under the recorder's wrapper, driven from this process.
//!
//! The server is started and relayed by [`mcp_trace_capture::stdio`], exactly
//! as `mcp-trace-capture stdio` starts and relays one; the only difference is
//! that the client's end is a pair of in-process pipes instead of this
//! process's stdin and stdout.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mcp_trace_capture::output::{self, Target};
use mcp_trace_capture::{DEFAULT_MAX_MESSAGE, Recorder, Signals, Stop, StopRequests, stdio};
use serde_json::Value;
use tokio::io::DuplexStream;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::task::JoinHandle;

use super::{Answer, Failure, Incoming, classify};
use crate::check::request::{Outgoing, Requests};

/// The capacity of each pipe to the relay. A capacity, not a limit: a longer
/// message waits for the relay to read.
const PIPE_BYTES: usize = 65_536;

/// How long a server gets to exit after its stdin closes before it is
/// stopped — the official SDKs' stdio clients wait the same.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// A running server and the pipes to it.
#[derive(Debug)]
pub(crate) struct Stdio {
    writer: DuplexStream,
    reader: BufReader<DuplexStream>,
    stop: StopRequests,
    relay: JoinHandle<()>,
    recorder: Arc<Recorder>,
}

impl Stdio {
    /// Starts `program` with `args` under the recorder, recording to `target`.
    ///
    /// # Errors
    ///
    /// The server cannot be started, or the trace cannot be created. The
    /// trace is created only once the server has started.
    pub(crate) fn start(
        program: &OsString,
        args: &[OsString],
        target: &Target,
        force: bool,
    ) -> Result<(Self, PathBuf), String> {
        // The recorder's error already names the program.
        let server = stdio::spawn(program, args).map_err(|error| error.to_string())?;
        let (recorder, path) = output::open(target, force, DEFAULT_MAX_MESSAGE, None)?;
        let (writer, from_client) = tokio::io::duplex(PIPE_BYTES);
        let (to_client, answers) = tokio::io::duplex(PIPE_BYTES);
        let (signals, stop) = Signals::requested();
        let relayed = Arc::clone(&recorder);
        let relay = tokio::spawn(async move {
            let outcome = stdio::relay(
                relayed,
                server,
                signals,
                from_client,
                to_client,
                DEFAULT_MAX_MESSAGE,
            )
            .await;
            if let Err(error) = outcome {
                eprintln!("mcp-conformance: waiting for the server failed: {error}");
            }
        });
        let stdio = Self {
            writer,
            reader: BufReader::new(answers),
            stop,
            relay,
            recorder,
        };
        Ok((stdio, path))
    }

    /// Writes `message` to the server as one line.
    pub(crate) async fn send(&mut self, message: &Value) -> Result<(), Failure> {
        let mut line = message.to_string();
        line.push('\n');
        let written = async {
            self.writer.write_all(line.as_bytes()).await?;
            self.writer.flush().await
        };
        // A pipe the relay has dropped means the server is gone.
        written.await.map_err(|_| Failure::Closed)
    }

    /// Sends `out` and reads until its answer.
    pub(crate) async fn exchange(
        &mut self,
        out: &Outgoing,
        requests: &Requests,
        max_response: usize,
    ) -> Result<Answer, Failure> {
        self.send(&out.body).await?;
        let mut server_requests = 0;
        loop {
            let mut line = Vec::new();
            let limit = u64::try_from(max_response)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            let read = (&mut self.reader)
                .take(limit)
                .read_until(b'\n', &mut line)
                .await
                .map_err(|error| Failure::Transport(format!("reading the server: {error}")))?;
            if read == 0 {
                return Err(Failure::Closed);
            }
            if line.last() != Some(&b'\n') && line.len() > max_response {
                return Err(Failure::TooLarge);
            }
            // A line that is not JSON is the server's to answer for; the
            // recorder has it, and the validator says so (BASE-008).
            let Ok(message) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            match classify(message, out.id) {
                Incoming::Answer(message) => {
                    return Ok(Answer {
                        message,
                        session: None,
                        server_requests,
                    });
                }
                Incoming::Request(request) => {
                    server_requests += 1;
                    self.send(&requests.answer(&request).body).await?;
                }
                Incoming::Other => {}
            }
        }
    }

    /// Closes the server's stdin, waits for it to exit (stopping it if it
    /// does not), and closes the trace.
    pub(crate) async fn close(self) -> Result<(), String> {
        let Self {
            writer,
            reader,
            stop,
            mut relay,
            recorder,
        } = self;
        drop(writer);
        if tokio::time::timeout(CLOSE_GRACE, &mut relay).await.is_err() {
            stop.request(Stop::Terminate);
            let _ = relay.await;
        }
        drop(reader);
        if recorder.finish().is_complete() {
            Ok(())
        } else {
            Err("a write to the trace failed, so it is incomplete".to_owned())
        }
    }
}
