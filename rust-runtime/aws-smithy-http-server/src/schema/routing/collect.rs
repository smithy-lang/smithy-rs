/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Incremental request-body collection for body-claiming protocols.

use crate::{
    error::BoxError,
    schema::RequestBodyCollectionConfig,
};
use bytes::Bytes;

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub(super) struct BodyCollector<B> {
    // The concrete pipeline body, unerased: `RequestBody<B>` is `Unpin` for the `B` the
    // service accepts, so frames are polled through `Pin::new`.
    body: crate::body::RequestBody<B>,
    // Raw wire chunks, in order. Zero-copy for a body delivering its content in one frame — a
    // buffered body always does; consolidated into one chunk lazily, when a contiguous view
    // is first needed.
    pub(super) chunks: Vec<Bytes>,
    pub(super) len: usize,
    trailers: Option<http::HeaderMap>,
    pub(super) eof: bool,
    config: RequestBodyCollectionConfig,
    // Armed on first poll and spanning the whole routing phase, so construction needs no
    // runtime context. Boxed to keep the collector `Unpin` and movable between walk states.
    deadline: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<B> BodyCollector<B> {
    pub(super) fn new(body: crate::body::RequestBody<B>, config: RequestBodyCollectionConfig) -> Self {
        Self {
            body,
            chunks: Vec::new(),
            len: 0,
            trailers: None,
            eof: false,
            config,
            deadline: None,
        }
    }

    /// A contiguous view of the first `upto` raw bytes. Zero-copy for a single chunk;
    /// otherwise the chunks consolidate once and stay consolidated.
    pub(super) fn contiguous(&mut self, upto: usize) -> Bytes {
        debug_assert!(upto <= self.len);
        if self.chunks.len() > 1 {
            let mut all = bytes::BytesMut::with_capacity(self.len);
            for chunk in self.chunks.drain(..) {
                all.extend_from_slice(&chunk);
            }
            self.chunks.push(all.freeze());
        }
        self.chunks.first().map_or_else(Bytes::new, |all| all.slice(..upto))
    }

    /// Rebuilds the request body for dispatch: everything read replays first, byte-identical,
    /// followed by the untouched transport remainder when the body was not read to its end.
    pub(super) fn into_replay_body(mut self) -> crate::body::RequestBody<B> {
        let raw = self.contiguous(self.len);
        if self.eof {
            crate::body::RequestBody::buffered(raw, self.trailers)
        } else {
            crate::body::RequestBody::prefixed(raw, self.trailers, self.body)
        }
    }
}

impl<B> BodyCollector<B>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    /// Advances collection by one step: appends a raw chunk, absorbs trailers, or observes
    /// end-of-body. `Ready(Ok(()))` means the state advanced; callers re-check their
    /// requirement against the buffer.
    pub(super) fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), crate::schema::RequestBodyCollectionError<crate::Error>>> {
        use crate::schema::RequestBodyCollectionError;
        use http_body::Body as _;
        if let Some(timeout) = self.config.read_timeout {
            let deadline = self
                .deadline
                .get_or_insert_with(|| Box::pin(tokio::time::sleep(timeout)));
            if deadline.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Err(RequestBodyCollectionError::Timeout { timeout }));
            }
        }
        if self.eof {
            return Poll::Ready(Ok(()));
        }
        match Pin::new(&mut self.body).poll_frame(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                self.eof = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Err(err))) => Poll::Ready(Err(RequestBodyCollectionError::Body(err))),
            Poll::Ready(Some(Ok(frame))) => {
                match frame.into_data() {
                    Ok(data) => {
                        if let Some(limit) = self.config.max_bytes {
                            if data.len() > limit.get().saturating_sub(self.len) {
                                return Poll::Ready(Err(RequestBodyCollectionError::TooLarge(
                                    crate::body::BodyLimitExceeded { limit: limit.get() },
                                )));
                            }
                        }
                        self.len += data.len();
                        self.chunks.push(data);
                    }
                    Err(frame) => {
                        if let Ok(new_trailers) = frame.into_trailers() {
                            self.trailers.get_or_insert_with(http::HeaderMap::new).extend(new_trailers);
                        }
                    }
                }
                Poll::Ready(Ok(()))
            }
        }
    }
}
