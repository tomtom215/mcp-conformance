// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Bodies as the proxy reads and relays them: the start of one read whole for
//! the trace, within the message limit and the proxy's buffer budget, and the
//! body relayed from it.
//!
//! **The budget.** Every byte the proxy holds to record — a body read whole, an
//! SSE event being assembled — is first taken from one [`Semaphore`] of
//! [`Options::buffer_budget`](super::Options::buffer_budget) permits, and given
//! back once the bytes are relayed. A body that would take more than is left is
//! not waited for: it is streamed on unrecorded, as a body over the message
//! limit is, and counted. So many large exchanges at once cost the budget, not
//! their number times the message limit.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use futures::{Stream, StreamExt as _};
use hyper::body::{Frame, SizeHint};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How a client's request body ends when [`idle_bounded`]'s bound passes: no
/// byte of it arrived for that long.
#[derive(Debug)]
pub(super) struct Stalled(Duration);

impl std::fmt::Display for Stalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no byte of the request body arrived for {}s",
            self.0.as_secs_f32()
        )
    }
}

impl std::error::Error for Stalled {}

/// Whether `error`, or any error behind it, is a [`Stalled`]: a body that
/// fails while hyper is sending it upstream comes back wrapped in hyper's own
/// error, with the cause behind it.
pub(super) fn is_stall(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.is::<Stalled>() {
            return true;
        }
        current = error.source();
    }
    false
}

/// `stream` with a bound on how long any one chunk may take to arrive after
/// the last: when `idle` passes with nothing read, the stream ends with a
/// [`Stalled`] error. For the client's request body only — an upstream's
/// response, an event stream above all, is open-ended by design.
pub(super) fn idle_bounded<S>(
    mut stream: S,
    idle: Duration,
) -> impl Stream<Item = Result<Bytes, axum::Error>> + Unpin
where
    S: Stream<Item = Result<Bytes, axum::Error>> + Unpin,
{
    let mut deadline = Box::pin(tokio::time::sleep(idle));
    let mut ended = false;
    futures::stream::poll_fn(move |context| {
        if ended {
            return Poll::Ready(None);
        }
        match stream.poll_next_unpin(context) {
            Poll::Ready(item) => {
                ended = item.is_none();
                deadline.as_mut().reset(tokio::time::Instant::now() + idle);
                Poll::Ready(item)
            }
            Poll::Pending => {
                if deadline.as_mut().poll(context).is_ready() {
                    ended = true;
                    return Poll::Ready(Some(Err(axum::Error::new(Stalled(idle)))));
                }
                Poll::Pending
            }
        }
    })
}

/// How a body read by [`read_prefix`] stopped.
#[derive(Debug)]
pub(super) enum End<E> {
    /// The body ended within the limit: the prefix is all of it.
    Complete,
    /// The body passed the limit; the rest is still in the stream.
    Over,
    /// The buffer budget ran out before the body ended; the rest is still in
    /// the stream.
    OverBudget,
    /// The body failed before any of those.
    Failed(E),
}

/// The start of a body, the budget it holds, and how reading it stopped.
#[derive(Debug)]
pub(super) struct Prefix<E> {
    pub(super) bytes: Vec<u8>,
    pub(super) held: Held,
    pub(super) end: End<E>,
}

/// Permits taken from the buffer budget, given back when dropped.
#[derive(Debug, Default)]
pub(super) struct Held(Option<OwnedSemaphorePermit>);

impl Held {
    /// How many bytes' worth this holds.
    pub(super) fn bytes(&self) -> usize {
        self.0.as_ref().map_or(0, OwnedSemaphorePermit::num_permits)
    }

    /// Makes this hold exactly `bytes`' worth, taking more from `budget` if
    /// need be. `false`, holding what it held, when the budget has not that
    /// much left.
    pub(super) fn resize(&mut self, budget: &Arc<Semaphore>, bytes: usize) -> bool {
        let have = self.bytes();
        if bytes > have {
            let Ok(more) = u32::try_from(bytes - have) else {
                return false;
            };
            let Ok(more) = Arc::clone(budget).try_acquire_many_owned(more) else {
                return false;
            };
            match &mut self.0 {
                Some(held) => held.merge(more),
                None => self.0 = Some(more),
            }
        } else if bytes == 0 {
            self.0 = None;
        } else if let Some(held) = &mut self.0 {
            drop(held.split(have - bytes));
        }
        true
    }
}

/// Reads until the body ends, fails, passes `max` bytes, or would take more of
/// `budget` than is left — keeping what was read in every case, and the budget
/// it took.
pub(super) async fn read_prefix<S, E>(
    stream: &mut S,
    max: usize,
    budget: &Arc<Semaphore>,
) -> Prefix<E>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    let mut bytes = Vec::new();
    let mut held = Held::default();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                return Prefix {
                    bytes,
                    held,
                    end: End::Failed(error),
                };
            }
        };
        // The chunk is in memory already: past the limit or the budget, it is
        // relayed with the rest rather than added to what is held. The limit
        // first, so a body over it is counted as such whatever the budget.
        let total = bytes.len() + chunk.len();
        // What is held is the buffer's capacity, which grows ahead of its
        // length: doubled, as a `Vec` grows, but never past the limit.
        let capacity = if total > bytes.capacity() {
            total
                .max(bytes.capacity().saturating_mul(2))
                .min(max.max(total))
        } else {
            bytes.capacity()
        };
        let end = if total > max {
            Some(End::Over)
        } else if held.resize(budget, capacity) {
            bytes.reserve_exact(capacity - bytes.len());
            None
        } else {
            Some(End::OverBudget)
        };
        bytes.extend_from_slice(&chunk);
        if let Some(end) = end {
            return Prefix { bytes, held, end };
        }
    }
    Prefix {
        bytes,
        held,
        end: End::Complete,
    }
}

/// A whole body of `bytes`, holding `held` until it has been relayed. Its
/// length is known, so it is sent with a `Content-Length`.
pub(super) fn whole(bytes: Vec<u8>, held: Held) -> Body {
    if bytes.is_empty() {
        return Body::empty();
    }
    Body::new(Whole {
        bytes: Some(Bytes::from(bytes)),
        _held: held,
    })
}

/// A body of `prefix` followed by whatever `rest` yields, holding `held` until
/// the prefix has been relayed.
pub(super) fn then_rest<S>(prefix: Vec<u8>, held: Held, rest: S) -> Body
where
    S: Stream<Item = Result<Bytes, axum::Error>> + Send + 'static,
{
    Body::from_stream(
        futures::stream::once(async move {
            // The prefix is relayed: its share of the budget is free again.
            drop(held);
            Ok::<_, axum::Error>(Bytes::from(prefix))
        })
        .chain(rest),
    )
}

/// [`whole`]'s body: one frame, of known length.
struct Whole {
    bytes: Option<Bytes>,
    _held: Held,
}

impl hyper::body::Body for Whole {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        Poll::Ready(self.bytes.take().map(|bytes| Ok(Frame::data(bytes))))
    }

    fn is_end_stream(&self) -> bool {
        self.bytes.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(
            self.bytes
                .as_ref()
                .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX)),
        )
    }
}

/// `stream` until `stop` resolves, between two chunks, then an error: hyper
/// aborts a response whose body fails rather than ending it, so the client sees
/// a stream that was cut — not one the server finished — and a proxy that is
/// stopping is not held open by it.
pub(super) fn cut_at_stop<S, F>(
    stream: S,
    stop: F,
) -> impl Stream<Item = Result<Bytes, axum::Error>>
where
    S: Stream<Item = Result<Bytes, axum::Error>>,
    F: std::future::Future<Output = ()>,
{
    use futures::FutureExt as _;
    let mut stream = Box::pin(stream);
    // Fused: once it has fired, polling it again is harmless.
    let mut stop = Box::pin(stop.fuse());
    futures::stream::poll_fn(move |context| {
        if stop.as_mut().poll(context).is_ready() {
            return Poll::Ready(Some(Err(axum::Error::new(std::io::Error::other(
                "the proxy is stopping",
            )))));
        }
        stream.as_mut().poll_next(context)
    })
}
