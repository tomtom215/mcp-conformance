// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reading a body for recording without ever changing what is delivered.

use axum::body::Body;
use tokio_stream::StreamExt as _;

/// A body read for recording.
pub(super) enum Buffered {
    /// The whole body, within [`MAX_RECORDED_BODY`](super::MAX_RECORDED_BODY).
    Whole(axum::body::Bytes),
    /// A body the tap will not record — over the cap, or cut off by an error —
    /// rebuilt to deliver exactly what the original would have: the bytes
    /// already read, then the rest of the stream (or its error), unchanged.
    Passed(Body),
}

/// Reads `body` up to [`MAX_RECORDED_BODY`](super::MAX_RECORDED_BODY). Never loses a byte: a body the tap
/// cannot record comes back as a stream of the same bytes.
pub(super) async fn buffer(body: Body) -> Buffered {
    let mut stream = body.into_data_stream();
    let mut read = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                read.extend_from_slice(&chunk);
                if read.len() > super::MAX_RECORDED_BODY {
                    let head = tokio_stream::once(Ok(axum::body::Bytes::from(read)));
                    return Buffered::Passed(Body::from_stream(head.chain(stream)));
                }
            }
            Err(error) => {
                let head = tokio_stream::once(Ok(axum::body::Bytes::from(read)));
                let failed = tokio_stream::once(Err(error));
                return Buffered::Passed(Body::from_stream(head.chain(failed).chain(stream)));
            }
        }
    }
    Buffered::Whole(axum::body::Bytes::from(read))
}
