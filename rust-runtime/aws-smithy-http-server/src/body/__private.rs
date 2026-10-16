/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Internal body helpers used by generated server code.
//!
//! This module is public so generated code can access its items. There are no guarantees
//! of public API stability: these items may change or be removed without a semver guarantee.
//!
//! Public items support generated trait bounds, body collection, and its error handling.
//! Streaming wrappers used only inside the runtime have narrower visibility.

use bon::Builder;
use bytes::{Buf, BufMut, Bytes};
use std::num::NonZeroUsize;

#[cfg(test)]
#[path = "tests/allocation_tests.rs"]
mod allocation_tests;

#[cfg(test)]
#[path = "tests/streaming_tests.rs"]
mod streaming_tests;

// Used in generated trait bounds.
pub use http_body::Body as HttpBody;

// ============================================================================
// Size-limited Body Collection
// ============================================================================

/// The error returned by [`collect_body`].
///
/// Either the underlying body produced an error, or the configured size limit was
/// exceeded. The generic over `E` lets the helper work with body error types that
/// are `Send` but not `Sync` (the generated-server code carries only a `Send` bound),
/// while still letting callers `?` the result into a `RequestRejection`.
#[derive(Debug, thiserror::Error)]
pub enum CollectBodyError<E> {
    /// The underlying body produced an error while being read.
    #[error("error reading request body: {0}")]
    Body(#[source] E),
    /// The body exceeded the configured maximum size.
    #[error("request body exceeded the configured maximum of {0} bytes")]
    BodyLimitExceeded(NonZeroUsize),
}

/// Default number of consecutive ready body polls before collection or streaming yields.
pub(crate) const DEFAULT_FRAME_BUDGET: NonZeroUsize = NonZeroUsize::new(32).unwrap();

/// Default cap on the buffer reserved before reading body frames.
pub(crate) const DEFAULT_MAX_INITIAL_CAPACITY: NonZeroUsize = NonZeroUsize::new(32 * 1024).unwrap();

/// Options for complete HTTP body collection.
#[derive(Debug, Builder)]
pub struct CollectionConfig {
    /// Maximum body bytes; `None` disables the byte limit.
    limit: Option<NonZeroUsize>,
    /// Number of ready frame polls before yielding.
    #[builder(default = DEFAULT_FRAME_BUDGET)]
    frame_budget: NonZeroUsize,
    /// Cap on the initial buffer reservation; zero starts without a reservation.
    #[builder(default = DEFAULT_MAX_INITIAL_CAPACITY.get())]
    initial_capacity: usize,
}

impl Default for CollectionConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Collect a body with explicit limits on bytes, ready frame polls, and initial reservation.
///
/// `initial_capacity` caps the buffer reserved from the body's size hint before frames
/// arrive. Passing `0` starts without a reservation. The buffer can grow beyond this cap as
/// data arrives, subject to `limit`; it is not a total-body limit.
///
/// `frame_budget` counts ready frame polls, including empty frames and trailers.
/// A pending body poll resets the budget; exhausting it yields and wakes the task.
/// Passing `None` uses no byte limit, a 32-frame budget, and a 32 KiB reservation cap.
pub async fn collect_body<B>(body: B, config: Option<CollectionConfig>) -> Result<Bytes, CollectBodyError<B::Error>>
where
    B: HttpBody,
{
    collect_body_with_trailers(body, config).await.map(|(bytes, _)| bytes)
}

fn new_collection_buffer(size_hint_lower: u64, max_initial_capacity: usize) -> bytes::BytesMut {
    bytes::BytesMut::with_capacity(size_hint_lower.min(max_initial_capacity as u64) as usize)
}

fn enforce_body_limit<E>(current_size: u64, limit: NonZeroUsize) -> Result<(), CollectBodyError<E>> {
    if current_size > limit.get() as u64 {
        tracing::trace!(
            limit = limit.get(),
            size_hint_lower = current_size,
            "request body limit exceeded"
        );
        return Err(CollectBodyError::BodyLimitExceeded(limit));
    }
    Ok(())
}

/// Collect a complete body, retaining trailers for callers that replay it.
pub async fn collect_body_with_trailers<B>(
    body: B,
    config: Option<CollectionConfig>,
) -> Result<(Bytes, Option<http::HeaderMap>), CollectBodyError<B::Error>>
where
    B: HttpBody,
{
    let config = config.unwrap_or_default();

    // Walk frames ourselves (rather than using `http_body_util::Limited`) so we
    // don't require `B::Error: Send + Sync + 'static`. The generated server
    // deserializer only carries a `Send` bound on body errors.
    let lower = body.size_hint().lower();
    if let Some(limit) = config.limit {
        enforce_body_limit(lower, limit)?;
    }

    let mut body = std::pin::pin!(wrap_body_with_frame_budget(body, config.frame_budget));

    // A size hint may come from an untrusted Content-Length. Cap the initial reservation;
    // larger buffers grow only as data arrives, subject to the total-body limit.
    let mut collected = new_collection_buffer(lower, config.initial_capacity);
    let mut trailers = None;
    loop {
        let next_frame = std::future::poll_fn(|cx| body.as_mut().poll_frame(cx)).await;

        // None means the body has ended.
        let Some(frame) = next_frame else {
            break;
        };

        // Propagate an error from the underlying body.
        let frame = frame.map_err(CollectBodyError::Body)?;

        match frame.into_data() {
            Ok(data) => {
                let data_len = data.remaining();
                if let Some(limit) = config.limit {
                    enforce_body_limit(collected.len().saturating_add(data_len) as u64, limit)?;
                }
                collected.put(data);
            }
            Err(frame) => {
                if let Ok(headers) = frame.into_trailers() {
                    trailers.get_or_insert_with(http::HeaderMap::new).extend(headers);
                }
            }
        }
    }

    Ok((collected.freeze(), trailers))
}

// ============================================================================
// Body Poll Budget
// ============================================================================

/// Default number of consecutive ready body polls before a streaming request yields.
pub(crate) const DEFAULT_STREAMING_FRAME_BUDGET: NonZeroUsize = DEFAULT_FRAME_BUDGET;

pin_project_lite::pin_project! {
    /// An HTTP body that yields after a bounded number of consecutive ready polls.
    ///
    /// Frames, errors, end-of-stream state, and size hints are forwarded from the inner body.
    #[derive(Debug)]
    pub(crate) struct BudgetedBody<B> {
        #[pin]
        inner: B,
        frame_budget: NonZeroUsize,
        remaining_budget: usize,
    }
}

/// Wraps a streaming request body with the default 32-poll budget.
pub(crate) fn wrap_streaming_body<B: HttpBody>(body: B) -> BudgetedBody<B> {
    wrap_body_with_frame_budget(body, DEFAULT_STREAMING_FRAME_BUDGET)
}

/// Wraps an HTTP body with a positive budget of consecutive ready frame polls.
///
/// Empty data frames and trailers count toward the budget. An underlying pending poll
/// resets it. Exhausting the budget yields without polling the inner body and wakes the
/// task to resume. This bounds work between yields, not the stream's total bytes or duration.
fn wrap_body_with_frame_budget<B: HttpBody>(body: B, frame_budget: NonZeroUsize) -> BudgetedBody<B> {
    BudgetedBody {
        inner: body,
        frame_budget,
        remaining_budget: frame_budget.get(),
    }
}

impl<B: HttpBody> HttpBody for BudgetedBody<B> {
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        if *this.remaining_budget == 0 {
            *this.remaining_budget = this.frame_budget.get();
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        }

        *this.remaining_budget -= 1;
        let result = this.inner.poll_frame(cx);
        if result.is_pending() {
            *this.remaining_budget = this.frame_budget.get();
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}
