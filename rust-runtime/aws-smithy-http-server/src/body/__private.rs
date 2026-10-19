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

/// Default maximum body bytes; `None` disables the byte limit.
pub(crate) const DEFAULT_LIMIT_SIZE: Option<NonZeroUsize> = None;

/// Default number of ready frame polls before body collection yields.
pub(crate) const DEFAULT_FRAME_BUDGET: NonZeroUsize = NonZeroUsize::new(32).unwrap();

/// Default cap on the buffer reserved before reading body frames.
pub(crate) const DEFAULT_MAX_INITIAL_CAPACITY: NonZeroUsize = NonZeroUsize::new(32 * 1024).unwrap();

/// Options for complete HTTP body collection.
#[derive(Debug)]
pub struct CollectionConfig {
    /// Maximum body bytes; `None` disables the byte limit.
    limit: Option<NonZeroUsize>,
    /// Number of ready frame polls before yielding.
    frame_budget: NonZeroUsize,
    /// Cap on the initial buffer reservation; `None` starts without a reservation.
    initial_capacity: Option<NonZeroUsize>,
}

impl CollectionConfig {
    /// Creates a builder with the default body collection options.
    pub fn builder() -> CollectionConfigBuilder {
        CollectionConfigBuilder::default()
    }

    /// Returns the maximum body bytes; `None` means no byte limit.
    pub fn limit(&self) -> Option<NonZeroUsize> {
        self.limit
    }

    /// Returns the number of ready frame polls before yielding.
    pub fn frame_budget(&self) -> NonZeroUsize {
        self.frame_budget
    }

    /// Returns the initial reservation cap; `None` means no initial reservation.
    pub fn initial_capacity(&self) -> Option<NonZeroUsize> {
        self.initial_capacity
    }
}

impl Default for CollectionConfig {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT_SIZE,
            frame_budget: DEFAULT_FRAME_BUDGET,
            initial_capacity: Some(DEFAULT_MAX_INITIAL_CAPACITY),
        }
    }
}

/// Builder for complete HTTP body collection options.
#[derive(Debug, Default)]
pub struct CollectionConfigBuilder {
    config: CollectionConfig,
}

impl CollectionConfigBuilder {
    /// Sets the maximum number of body bytes.
    pub fn limit(self, limit: NonZeroUsize) -> Self {
        self.maybe_limit(Some(limit))
    }

    /// Sets the byte limit, or disables it with `None`.
    pub fn maybe_limit(mut self, limit: Option<NonZeroUsize>) -> Self {
        self.config.limit = limit;
        self
    }

    /// Sets the number of ready frame polls before yielding.
    pub fn frame_budget(mut self, frame_budget: NonZeroUsize) -> Self {
        self.config.frame_budget = frame_budget;
        self
    }

    /// Sets the frame poll budget, or uses the default with `None`.
    pub fn maybe_frame_budget(self, frame_budget: Option<NonZeroUsize>) -> Self {
        self.frame_budget(frame_budget.unwrap_or(DEFAULT_FRAME_BUDGET))
    }

    /// Sets the initial buffer reservation cap.
    pub fn initial_capacity(self, initial_capacity: NonZeroUsize) -> Self {
        self.maybe_initial_capacity(Some(initial_capacity))
    }

    /// Sets the reservation cap, or disables the initial reservation with `None`.
    pub fn maybe_initial_capacity(mut self, initial_capacity: Option<NonZeroUsize>) -> Self {
        self.config.initial_capacity = initial_capacity;
        self
    }

    /// Builds the body collection options.
    pub fn build(self) -> CollectionConfig {
        self.config
    }
}

/// Collect a body with explicit limits on bytes, ready frame polls, and initial reservation.
/// Trailer frames are consumed and discarded without accumulating a header map.
///
/// `initial_capacity` caps the buffer reserved from the body's size hint before frames
/// arrive. Setting it to `None` starts without a reservation. The buffer can grow beyond this cap as
/// data arrives, subject to `limit`; it is not a total-body limit.
///
/// `frame_budget` counts ready frame polls, including empty frames and trailers.
/// A pending body poll resets the budget; exhausting it yields and wakes the task.
/// Passing `None` uses no byte limit, a 32-frame budget, and a 32 KiB reservation cap.
pub async fn collect_body<B>(body: B, config: Option<CollectionConfig>) -> Result<Bytes, CollectBodyError<B::Error>>
where
    B: HttpBody,
{
    CollectBodyFuture::<B, false>::new(body, config)
        .await
        .map(|(bytes, _)| bytes)
}

/// Collect a complete body, retaining trailers for callers that replay it.
pub async fn collect_body_with_trailers<B>(
    body: B,
    config: Option<CollectionConfig>,
) -> Result<(Bytes, Option<http::HeaderMap>), CollectBodyError<B::Error>>
where
    B: HttpBody,
{
    CollectBodyFuture::<B, true>::new(body, config).await
}

pin_project_lite::pin_project! {
    /// Complete-body collection with lazy reservation and pinned body ownership.
    pub(crate) struct BodyCollectionFuture<B, R> {
        #[pin]
        body: Option<BudgetedBody<B>>,
        config: CollectionConfig,
        collected: Option<bytes::BytesMut>,
        trailers: Option<http::HeaderMap>,
        done: bool,
        retention: std::marker::PhantomData<R>,
    }
}

// pin-project-lite projects type parameters; the marker carries the trailer policy.
pub(crate) struct TrailerRetention<const RETAIN_TRAILERS: bool>;
pub(crate) type CollectBodyFuture<B, const RETAIN_TRAILERS: bool> =
    BodyCollectionFuture<B, TrailerRetention<RETAIN_TRAILERS>>;

impl<B: HttpBody, const RETAIN_TRAILERS: bool> CollectBodyFuture<B, RETAIN_TRAILERS> {
    pub(crate) fn new(body: B, config: Option<CollectionConfig>) -> Self {
        let config = config.unwrap_or_default();
        Self {
            body: Some(wrap_body_with_frame_budget(body, config.frame_budget)),
            config,
            collected: None,
            trailers: None,
            done: false,
            retention: std::marker::PhantomData,
        }
    }
}

impl<B: HttpBody, const RETAIN_TRAILERS: bool> std::future::Future for CollectBodyFuture<B, RETAIN_TRAILERS> {
    type Output = Result<(Bytes, Option<http::HeaderMap>), CollectBodyError<B::Error>>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        use std::task::Poll;
        let mut this = self.project();
        assert!(!*this.done, "polled after completion");
        if this.collected.is_none() {
            let lower = this
                .body
                .as_ref()
                .get_ref()
                .as_ref()
                .expect("body remains until completion")
                .size_hint()
                .lower();
            if let Some(limit) = this.config.limit {
                if let Err(error) = enforce_body_limit(lower, limit) {
                    *this.done = true;
                    this.body.set(None);
                    return Poll::Ready(Err(error));
                }
            }
            // Reserve only on first poll. Size hints may reflect untrusted Content-Length.
            *this.collected = Some(new_collection_buffer(lower, this.config.initial_capacity));
        }
        loop {
            // BudgetedBody preserves the existing ready-frame count and pending reset.
            let frame = match this
                .body
                .as_mut()
                .as_pin_mut()
                .expect("body remains until completion")
                .poll_frame(cx)
            {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    *this.done = true;
                    this.body.set(None);
                    return Poll::Ready(Ok((
                        this.collected.take().expect("initialized above").freeze(),
                        this.trailers.take(),
                    )));
                }
                Poll::Ready(Some(Err(error))) => {
                    *this.done = true;
                    this.body.set(None);
                    *this.collected = None;
                    *this.trailers = None;
                    return Poll::Ready(Err(CollectBodyError::Body(error)));
                }
                Poll::Ready(Some(Ok(frame))) => frame,
            };
            match frame.into_data() {
                Ok(data) => {
                    let collected = this.collected.as_mut().expect("initialized above");
                    if let Some(limit) = this.config.limit {
                        if let Err(error) =
                            enforce_body_limit(collected.len().saturating_add(data.remaining()) as u64, limit)
                        {
                            *this.done = true;
                            this.body.set(None);
                            *this.collected = None;
                            *this.trailers = None;
                            return Poll::Ready(Err(error));
                        }
                    }
                    collected.put(data);
                }
                Err(frame) if RETAIN_TRAILERS => {
                    if let Ok(headers) = frame.into_trailers() {
                        this.trailers.get_or_insert_with(http::HeaderMap::new).extend(headers);
                    }
                }
                Err(_) => {}
            }
        }
    }
}

fn new_collection_buffer(size_hint_lower: u64, max_initial_capacity: Option<NonZeroUsize>) -> bytes::BytesMut {
    match max_initial_capacity {
        Some(maximum) => bytes::BytesMut::with_capacity(size_hint_lower.min(maximum.get() as u64) as usize),
        None => bytes::BytesMut::new(),
    }
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
