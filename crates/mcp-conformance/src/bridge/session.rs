// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One bridged session: a stdio server process under the recorder's stdio
//! wrapper, and where each line it writes goes.
//!
//! Over stdio a server's message carries no address; over streamable HTTP each
//! travels on a stream. The rule, the one the transport itself suggests:
//!
//! - a **response** goes on the stream of the POST that carried its request,
//!   and that stream ends once every request it carried is answered;
//! - a **request or notification** from the server (progress, logging,
//!   `sampling/createMessage`, `elicitation/create`) goes on the newest POST
//!   stream still open — the request in flight it most plausibly belongs to —
//!   else on the client's standalone GET stream, else it waits for one;
//! - a line that is **not JSON** is in the trace (the validator judges it) and
//!   is not forwarded: no streamable-HTTP client could read it.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, DuplexStream};
use tokio::sync::mpsc;

/// How many server messages wait for a stream before more are dropped.
const PENDING_LIMIT: usize = 256;

/// A stream's sending side: each item is one line the server wrote.
pub(super) type Sender = mpsc::UnboundedSender<String>;

/// The server side of a session.
#[derive(Debug)]
pub(super) struct Session {
    /// What the client sends, towards the server; `None` once closed.
    writer: tokio::sync::Mutex<Option<DuplexStream>>,
    routes: Mutex<Routes>,
}

#[derive(Debug, Default)]
struct Routes {
    /// POST streams still open, oldest first.
    streams: Vec<Stream>,
    /// The client's standalone GET stream.
    standalone: Option<Sender>,
    /// Server messages no stream could carry yet.
    pending: VecDeque<String>,
    /// The server's output has ended: nothing more will be routed.
    ended: bool,
}

#[derive(Debug)]
struct Stream {
    /// The ids (as JSON text) of requests this POST carried, not yet answered.
    waiting: Vec<String>,
    sender: Sender,
}

impl Session {
    /// A session writing the client's lines to `writer`.
    pub(super) fn new(writer: DuplexStream) -> Self {
        Self {
            writer: tokio::sync::Mutex::new(Some(writer)),
            routes: Mutex::new(Routes::default()),
        }
    }

    /// Writes one client message, framed as a line, towards the server.
    ///
    /// # Errors
    ///
    /// The session is closed, or the server's input is gone.
    pub(super) async fn send(&self, line: &[u8]) -> std::io::Result<()> {
        let mut guard = self.writer.lock().await;
        let Some(writer) = guard.as_mut() else {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        };
        let written = match writer.write_all(line).await {
            Ok(()) => writer.flush().await,
            Err(error) => Err(error),
        };
        drop(guard);
        written
    }

    /// Closes the client's side: the server reads end of stream, as a stdio
    /// client closing its pipe.
    pub(super) async fn close(&self) {
        self.writer.lock().await.take();
    }

    /// A stream for a POST that carried requests with `ids`: it receives their
    /// responses, and server messages sent while it is the newest open, and
    /// ends once every id is answered (or the server's output ends).
    pub(super) fn open_stream(&self, ids: Vec<String>) -> mpsc::UnboundedReceiver<String> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut routes = self.lock();
        if !routes.ended {
            routes.streams.push(Stream {
                waiting: ids,
                sender,
            });
        }
        receiver
    }

    /// The client's standalone stream: whatever waited for one arrives first.
    pub(super) fn open_standalone(&self) -> mpsc::UnboundedReceiver<String> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut routes = self.lock();
        if !routes.ended {
            for line in routes.pending.drain(..) {
                let _ = sender.send(line);
            }
            routes.standalone = Some(sender);
        }
        receiver
    }

    /// Routes one line the server wrote.
    pub(super) fn route(&self, line: String) {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            eprintln!(
                "mcp-conformance: the stdio server wrote a line that is not JSON; it is in \
                 the trace, for the validator to judge, and was not forwarded"
            );
            return;
        };
        let mut routes = self.lock();
        if let Some(id) = response_id(&message)
            && let Some(index) = routes
                .streams
                .iter()
                .position(|stream| stream.waiting.contains(&id))
        {
            let stream = &mut routes.streams[index];
            stream.waiting.retain(|waiting| *waiting != id);
            let _ = stream.sender.send(line);
            if stream.waiting.is_empty() {
                routes.streams.remove(index);
            }
            return;
        }
        routes.unsolicited(line);
    }

    /// The server's output ended: every stream ends with it.
    pub(super) fn end(&self) {
        let mut routes = self.lock();
        routes.ended = true;
        routes.streams.clear();
        routes.standalone = None;
        routes.pending.clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Routes> {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Routes {
    fn unsolicited(&mut self, line: String) {
        // Streams whose client went away are closed; skip them.
        self.streams.retain(|stream| !stream.sender.is_closed());
        let line = match self.streams.last() {
            Some(stream) => match stream.sender.send(line) {
                Ok(()) => return,
                Err(mpsc::error::SendError(line)) => line,
            },
            None => line,
        };
        let line = match &self.standalone {
            Some(standalone) => match standalone.send(line) {
                Ok(()) => return,
                Err(mpsc::error::SendError(line)) => {
                    self.standalone = None;
                    line
                }
            },
            None => line,
        };
        if self.pending.len() < PENDING_LIMIT {
            self.pending.push_back(line);
        } else {
            eprintln!(
                "mcp-conformance: a server message had no stream to travel on and \
                 {PENDING_LIMIT} were already waiting; it is in the trace, not forwarded"
            );
        }
    }
}

/// Reads the server's lines from `answers` and routes each, until its output
/// ends.
pub(super) async fn route_output(session: std::sync::Arc<Session>, answers: DuplexStream) {
    let mut lines = tokio::io::BufReader::new(answers).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        session.route(line);
    }
    session.end();
}

/// The id of a response (a message with `result` or `error` and no `method`),
/// as JSON text.
pub(super) fn response_id(message: &serde_json::Value) -> Option<String> {
    let object = message.as_object()?;
    if object.contains_key("method")
        || !(object.contains_key("result") || object.contains_key("error"))
    {
        return None;
    }
    object.get("id").map(ToString::to_string)
}

/// The ids, as JSON text, of the requests in a client message: one, or each
/// in a batch.
pub(super) fn request_ids(message: &serde_json::Value) -> Vec<String> {
    let one = |message: &serde_json::Value| {
        let object = message.as_object()?;
        object.get("method")?;
        object.get("id").map(ToString::to_string)
    };
    match message {
        serde_json::Value::Array(batch) => batch.iter().filter_map(one).collect(),
        message => one(message).into_iter().collect(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
