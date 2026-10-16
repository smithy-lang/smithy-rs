/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP body utilities.
//!
//! This module provides body handling utilities for HTTP 1.x using the
//! `http-body` and `http-body-util` crates.

use crate::error::{BoxError, Error};
use bytes::Bytes;

/// Internal helpers used by generated server code.
///
/// Public so generated code can access them. No public API stability or semver guarantees.
#[doc(hidden)]
pub mod __private;

// Compatibility aliases for previously generated crates; new code uses __private.
#[doc(hidden)]
pub use __private::{collect_body, CollectBodyError, CollectionConfig, HttpBody};

// ============================================================================
// BoxBody - Type-Erased Body
// ============================================================================

/// The primary body type returned by the generated `smithy-rs` service.
///
/// This is a type-erased body that wraps `UnsyncBoxBody` from `http-body-util`.
/// It is `Send` but not `Sync`, making it suitable for most HTTP handlers.
pub type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, Error>;

/// A thread-safe body type for operations that require `Sync`.
///
/// This is used specifically for event streaming operations and lambda handlers
/// that need thread safety guarantees.
pub type BoxBodySync = http_body_util::combinators::BoxBody<Bytes, Error>;

/// A request body on the schema-routing pipeline.
///
/// A request entering through hyper keeps its [`Incoming`](hyper::body::Incoming) body
/// inside the `Hyper` state, without boxing. The routing service replaces it with the
/// `Buffered` state after collecting for a body-first protocol. Other compatible HTTP bodies
/// enter through [`RequestBody::new`], which erases them into a boxed state.
///
/// [`RequestBody::prefixed`] replays bytes already read ahead of the remaining body. This allows
/// middleware to inspect a prefix, then forward the full request without collecting the rest.
///
/// The states are private so buffering and transport-specific optimizations can evolve.
///
/// `RequestBody` is `Send` and `Sync`, so it can be handed to [`SdkBody::from_body_1_x`]
/// when an operation input has a streaming member; `SdkBody` only accepts `Sync` bodies.
///
/// [`SdkBody::from_body_1_x`]: aws_smithy_types::body::SdkBody::from_body_1_x
#[derive(Debug)]
pub struct RequestBody(BodyInner);

/// The default request body: the schema pipeline over hyper's transport body.
pub type Body = RequestBody;

#[derive(Debug)]
enum BodyInner {
    /// A hyper incoming body. May have been partially polled.
    Hyper(hyper::body::Incoming),
    /// An erased body from a non-transport source. May have been partially polled.
    Boxed(BoxBodySync),
    /// Buffered bytes, followed by an optional unread tail and any already-captured trailers.
    Buffered {
        bytes: Option<Bytes>,
        trailers: Option<http::HeaderMap>,
        // None means complete collection. A tail remains until polling it returns Ready(None).
        tail: Option<Box<RequestBody>>,
    },
}

impl RequestBody {
    /// Wraps any compatible HTTP body without polling it.
    ///
    /// A hyper incoming body and an already-wrapped `RequestBody` stay unerased; any other
    /// body type is erased into a boxed state.
    pub fn new<T>(body: T) -> Self
    where
        T: http_body::Body<Data = Bytes> + Send + Sync + 'static,
        T::Error: Into<BoxError>,
    {
        try_downcast::<Self, T>(body)
            .or_else(|body| try_downcast::<hyper::body::Incoming, T>(body).map(|body| Self(BodyInner::Hyper(body))))
            .unwrap_or_else(|body| Self(BodyInner::Boxed(boxed_sync(body))))
    }

    /// Builds an already-buffered body, preserving any trailers read with the content.
    pub(crate) fn buffered(bytes: Bytes, trailers: Option<http::HeaderMap>) -> Self {
        Self(BodyInner::Buffered {
            bytes: Some(bytes),
            trailers,
            tail: None,
        })
    }

    /// Replays bytes already read from a request ahead of its remaining body.
    ///
    /// Include all consumed data in `prefix` so downstream readers receive the full request.
    /// Construction does not poll or erase `tail`. An empty prefix with no captured trailers
    /// returns `tail` unchanged.
    ///
    /// Pass `None` for `trailers` when only data has been read: unread trailers arrive from
    /// `tail` normally. If trailer frames were already read, pass them here to replay them
    /// after `tail` reaches end-of-stream.
    pub fn prefixed(prefix: Bytes, trailers: Option<http::HeaderMap>, tail: Self) -> Self {
        if prefix.is_empty() && trailers.is_none() {
            return tail;
        }
        Self(BodyInner::Buffered {
            bytes: Some(prefix).filter(|bytes| !bytes.is_empty()),
            trailers,
            tail: Some(Box::new(tail)),
        })
    }

    // Only unpolled, completely buffered bodies qualify for complete-content reuse.
    pub(crate) fn buffered_content(&self) -> Option<&Bytes> {
        match &self.0 {
            BodyInner::Buffered { bytes, tail: None, .. } => bytes.as_ref(),
            _ => None,
        }
    }

    /// Creates an empty request body.
    pub fn empty() -> Self {
        Self(BodyInner::Buffered {
            bytes: None,
            trailers: None,
            tail: None,
        })
    }

    /// Creates a request body containing buffered bytes.
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self::buffered(bytes, None)
    }
}

impl Default for RequestBody {
    fn default() -> Self {
        Self::empty()
    }
}

/// The transport body enters unerased, in the `Hyper` state.
///
/// Other compatible HTTP bodies can be wrapped with [`RequestBody::new`].
impl From<hyper::body::Incoming> for Body {
    fn from(body: hyper::body::Incoming) -> Self {
        Self(BodyInner::Hyper(body))
    }
}

impl From<Bytes> for RequestBody {
    fn from(bytes: Bytes) -> Self {
        Self::from_bytes(bytes)
    }
}

impl From<Vec<u8>> for RequestBody {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_bytes(bytes.into())
    }
}

impl From<&'static [u8]> for RequestBody {
    fn from(bytes: &'static [u8]) -> Self {
        Self::from_bytes(Bytes::from_static(bytes))
    }
}

impl From<String> for RequestBody {
    fn from(string: String) -> Self {
        Self::from_bytes(string.into())
    }
}

impl From<&'static str> for RequestBody {
    fn from(string: &'static str) -> Self {
        Self::from_bytes(Bytes::from_static(string.as_bytes()))
    }
}

impl http_body::Body for RequestBody {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Bytes>, Error>>> {
        match &mut self.0 {
            BodyInner::Hyper(body) => std::pin::Pin::new(body)
                .poll_frame(cx)
                .map(|frame| frame.map(|result| result.map_err(Error::new))),
            BodyInner::Boxed(body) => std::pin::Pin::new(body).poll_frame(cx),
            BodyInner::Buffered { bytes, trailers, tail } => {
                if let Some(bytes) = bytes.take().filter(|bytes| !bytes.is_empty()) {
                    return std::task::Poll::Ready(Some(Ok(http_body::Frame::data(bytes))));
                }
                if let Some(remaining) = tail {
                    match std::pin::Pin::new(remaining.as_mut()).poll_frame(cx) {
                        std::task::Poll::Ready(None) => *tail = None,
                        other => return other,
                    }
                }
                std::task::Poll::Ready(trailers.take().map(http_body::Frame::trailers).map(Ok))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.0 {
            BodyInner::Hyper(body) => body.is_end_stream(),
            BodyInner::Boxed(body) => body.is_end_stream(),
            BodyInner::Buffered { bytes, trailers, tail } => {
                bytes.as_ref().is_none_or(|bytes| bytes.is_empty())
                    && tail.as_ref().is_none_or(|tail| tail.is_end_stream())
                    && trailers.is_none()
            }
        }
    }
    fn size_hint(&self) -> http_body::SizeHint {
        match &self.0 {
            BodyInner::Hyper(body) => body.size_hint(),
            BodyInner::Boxed(body) => body.size_hint(),
            BodyInner::Buffered { bytes, tail, .. } => {
                let prefix = bytes.as_ref().map_or(0, |bytes| bytes.len() as u64);
                let remaining = tail
                    .as_ref()
                    .map_or_else(|| http_body::SizeHint::with_exact(0), |tail| tail.size_hint());
                let mut hint = http_body::SizeHint::new();
                hint.set_lower(prefix.saturating_add(remaining.lower()));
                if let Some(upper) = remaining.upper().and_then(|upper| prefix.checked_add(upper)) {
                    hint.set_upper(upper);
                }
                hint
            }
        }
    }
}

// ============================================================================
// Body Construction Functions
// ============================================================================

// `boxed` is used in the codegen of the implementation of the operation `Handler` trait.
/// Convert an HTTP body implementing [`http_body::Body`] into a [`BoxBody`].
pub fn boxed<B>(body: B) -> BoxBody
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    use http_body_util::BodyExt;

    try_downcast(body).unwrap_or_else(|body| body.map_err(Error::new).boxed_unsync())
}

/// Convert an HTTP body implementing [`http_body::Body`] into a [`BoxBodySync`].
pub fn boxed_sync<B>(body: B) -> BoxBodySync
where
    B: http_body::Body<Data = Bytes> + Send + Sync + 'static,
    B::Error: Into<BoxError>,
{
    use http_body_util::BodyExt;
    body.map_err(Error::new).boxed()
}

/// Convert bytes or similar types into a [`BoxBody`].
pub fn to_boxed<B>(body: B) -> BoxBody
where
    B: Into<Bytes>,
{
    boxed(http_body_util::Full::new(body.into()))
}

/// Convert bytes or similar types into a [`BoxBodySync`].
pub fn to_boxed_sync<B>(body: B) -> BoxBodySync
where
    B: Into<Bytes>,
{
    boxed_sync(http_body_util::Full::new(body.into()))
}

fn try_downcast<T, K>(k: K) -> Result<T, K>
where
    T: 'static,
    K: Send + 'static,
{
    let mut k = Some(k);
    if let Some(k) = <dyn std::any::Any>::downcast_mut::<Option<T>>(&mut k) {
        Ok(k.take().unwrap())
    } else {
        Err(k.unwrap())
    }
}

/// Create an empty body.
pub fn empty() -> BoxBody {
    boxed(http_body_util::Empty::<Bytes>::new())
}

/// Create an empty sync body.
pub fn empty_sync() -> BoxBodySync {
    boxed_sync(http_body_util::Empty::<Bytes>::new())
}

/// Create a body from bytes.
pub fn from_bytes(bytes: Bytes) -> BoxBody {
    boxed(http_body_util::Full::new(bytes))
}

// ============================================================================
// Stream Wrapping for Event Streaming
// ============================================================================

/// Wrap a stream of byte chunks into a BoxBody.
///
/// This is used for event streaming support. The stream should produce `Result<O, E>`
/// where `O` can be converted into `Bytes` and `E` can be converted into an error.
///
/// In hyper 0.x, `Body::wrap_stream` was available directly on the body type.
/// In hyper 1.x, the `stream` feature was removed, and the official approach is to use
/// `http_body_util::StreamBody` to convert streams into bodies, which is what this
/// function provides as a convenient wrapper.
///
/// For scenarios requiring `Sync` (e.g., lambda handlers), use [`wrap_stream_sync`] instead.
pub fn wrap_stream<S, O, E>(stream: S) -> BoxBody
where
    S: futures_util::Stream<Item = Result<O, E>> + Send + 'static,
    O: Into<Bytes> + 'static,
    E: Into<BoxError> + 'static,
{
    use futures_util::TryStreamExt;
    use http_body_util::StreamBody;

    // Convert the stream of Result<O, E> into a stream of Result<Frame<Bytes>, Error>
    let frame_stream = stream
        .map_ok(|chunk| http_body::Frame::data(chunk.into()))
        .map_err(|e| Error::new(e.into()));

    boxed(StreamBody::new(frame_stream))
}

/// Wrap a stream of byte chunks into a BoxBodySync.
///
/// This is the thread-safe variant of [`wrap_stream`], used for event streaming operations
/// that require `Sync` bounds, such as lambda handlers.
///
/// The stream should produce `Result<O, E>` where `O` can be converted into `Bytes` and
/// `E` can be converted into an error.
pub fn wrap_stream_sync<S, O, E>(stream: S) -> BoxBodySync
where
    S: futures_util::Stream<Item = Result<O, E>> + Send + Sync + 'static,
    O: Into<Bytes> + 'static,
    E: Into<BoxError> + 'static,
{
    use futures_util::TryStreamExt;
    use http_body_util::StreamBody;

    // Convert the stream of Result<O, E> into a stream of Result<Frame<Bytes>, Error>
    let frame_stream = stream
        .map_ok(|chunk| http_body::Frame::data(chunk.into()))
        .map_err(|e| Error::new(e.into()));

    boxed_sync(StreamBody::new(frame_stream))
}

#[cfg(test)]
mod downcast_tests;

#[cfg(test)]
mod partial_buffer_tests;

#[cfg(test)]
mod tests {
    use super::__private::*;
    use super::*;
    use std::num::NonZeroUsize;

    #[tokio::test]
    async fn request_body_preserves_buffered_bytes_and_trailers() {
        use http_body_util::BodyExt;

        for body in [
            RequestBody::from(Bytes::from_static(b"data")),
            RequestBody::from(b"data".to_vec()),
        ] {
            let mut body = RequestBody::new(body);
            assert_eq!(body.buffered_content().unwrap(), "data");
            assert_eq!(body.size_hint().exact(), Some(4));
            assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "data");
            assert!(body.is_end_stream());
        }

        let mut trailers = http::HeaderMap::new();
        trailers.insert("x-checksum", http::HeaderValue::from_static("complete"));
        let mut body = RequestBody::new(RequestBody::buffered(
            Bytes::from_static(b"data"),
            Some(trailers.clone()),
        ));
        assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "data");
        assert!(!body.is_end_stream());
        assert_eq!(body.frame().await.unwrap().unwrap().into_trailers().unwrap(), trailers);
        assert!(body.is_end_stream());
        assert!(body.frame().await.is_none());
    }

    #[tokio::test]
    async fn request_body_boxes_custom_stream_without_polling_ahead() {
        use http_body_util::BodyExt;
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        use std::task::Poll;

        let polls = Arc::new(AtomicUsize::new(0));
        let observed = polls.clone();
        let frames = futures_util::stream::poll_fn(move |_| {
            Poll::Ready(match observed.fetch_add(1, Ordering::SeqCst) {
                0 => Some(Ok(http_body::Frame::data(Bytes::from_static(b"data")))),
                1 => Some(Err(Error::new(std::io::Error::other("stream failed")))),
                _ => None,
            })
        });
        let mut body = RequestBody::new(http_body_util::StreamBody::new(frames));
        assert!(matches!(&body.0, BodyInner::Boxed(_)));
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "data");
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        assert!(body.frame().await.unwrap().is_err());
        assert!(body.frame().await.is_none());
    }

    #[test]
    fn collection_errors_preserve_messages_and_sources() {
        use std::error::Error as _;

        let transport = CollectBodyError::Body(std::io::Error::other("transport failure"));
        assert_eq!(transport.to_string(), "error reading request body: transport failure");
        assert!(transport.source().unwrap().downcast_ref::<std::io::Error>().is_some());

        let limit = NonZeroUsize::new(1024).unwrap();
        let exceeded: CollectBodyError<std::io::Error> = CollectBodyError::BodyLimitExceeded(limit);
        assert_eq!(
            exceeded.to_string(),
            "request body exceeded the configured maximum of 1024 bytes"
        );
        assert!(exceeded.source().is_none());
        assert!(matches!(exceeded, CollectBodyError::BodyLimitExceeded(actual) if actual == limit));
    }

    #[test]
    fn collection_yields_and_self_wakes_with_default_and_explicit_frame_budgets() {
        use std::future::Future;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::task::{Context, Poll, Wake, Waker};

        struct WakeCount(AtomicUsize);
        impl Wake for WakeCount {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        for budget in [None, NonZeroUsize::new(1), NonZeroUsize::new(7)] {
            for limit in [0, 1024] {
                let polls = Arc::new(AtomicUsize::new(0));
                let observed = polls.clone();
                let frames = futures_util::stream::poll_fn(move |_| {
                    let index = observed.fetch_add(1, Ordering::SeqCst);
                    Poll::Ready(match index {
                        0 => Some(Ok::<_, Error>(http_body::Frame::data(Bytes::from_static(b"x")))),
                        1..=129 => Some(Ok(http_body::Frame::data(Bytes::new()))),
                        130 => Some(Ok(http_body::Frame::trailers(http::HeaderMap::new()))),
                        _ => None,
                    })
                });
                let body = http_body_util::StreamBody::new(frames);
                let mut future = Box::pin(async move {
                    collect_body(
                        body,
                        Some(
                            CollectionConfig::builder()
                                .maybe_limit(NonZeroUsize::new(limit))
                                .frame_budget(budget.unwrap_or(DEFAULT_FRAME_BUDGET))
                                .build(),
                        ),
                    )
                    .await
                });
                let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
                let waker = Waker::from(wakes.clone());
                let mut cx = Context::from_waker(&waker);
                let expected_budget = budget.map(NonZeroUsize::get).unwrap_or(DEFAULT_FRAME_BUDGET.get());
                let mut yields = 0;
                loop {
                    match future.as_mut().poll(&mut cx) {
                        Poll::Pending => {
                            yields += 1;
                            assert_eq!(polls.load(Ordering::SeqCst), yields * expected_budget);
                            assert_eq!(wakes.0.load(Ordering::SeqCst), yields);
                        }
                        Poll::Ready(result) => {
                            assert_eq!(result.unwrap(), "x");
                            assert_eq!(polls.load(Ordering::SeqCst), 132);
                            assert_eq!(yields, 131 / expected_budget);
                            break;
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn collection_resets_frame_budget_after_pending_and_counts_trailers() {
        use std::future::Future;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::task::{Context, Poll};

        let polls = Arc::new(AtomicUsize::new(0));
        let observed = polls.clone();
        let frames = futures_util::stream::poll_fn(move |cx| {
            let index = observed.fetch_add(1, Ordering::SeqCst);
            match index {
                0 | 2 | 4 => Poll::Ready(Some(Ok::<_, Error>(http_body::Frame::data(Bytes::from_static(b"x"))))),
                1 => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                3 => Poll::Ready(Some(Ok(http_body::Frame::trailers(http::HeaderMap::new())))),
                _ => Poll::Ready(None),
            }
        });
        let body = http_body_util::StreamBody::new(frames);
        let mut future = Box::pin(collect_body(
            body,
            Some(
                CollectionConfig::builder()
                    .frame_budget(NonZeroUsize::new(3).unwrap())
                    .build(),
            ),
        ));
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), 2);
        assert!(future.as_mut().poll(&mut cx).is_pending());
        // A fresh budget permits three more frames, including the trailer.
        assert_eq!(polls.load(Ordering::SeqCst), 5);
        let Poll::Ready(Ok(bytes)) = future.as_mut().poll(&mut cx) else {
            panic!("collection should finish after yielding");
        };
        assert_eq!(bytes, "xxx");
    }

    /// Collect all bytes from a body (test utility).
    ///
    /// This uses `http_body_util::BodyExt::collect()` to read all body chunks
    /// into a single `Bytes` buffer.
    async fn collect_bytes<B>(body: B) -> Result<Bytes, Error>
    where
        B: HttpBody,
        B::Error: Into<BoxError>,
    {
        use http_body_util::BodyExt;

        let collected = body.collect().await.map_err(Error::new)?;
        Ok(collected.to_bytes())
    }

    #[tokio::test]
    async fn test_empty_body() {
        let body = empty();
        let bytes = collect_bytes(body).await.unwrap();
        assert_eq!(bytes.len(), 0);
    }

    #[tokio::test]
    async fn test_from_bytes() {
        let data = Bytes::from("hello world");
        let body = from_bytes(data.clone());
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, data);
    }

    #[tokio::test]
    async fn test_to_boxed_string() {
        let s = "hello world";
        let body = to_boxed(s);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from(s));
    }

    #[tokio::test]
    async fn test_to_boxed_vec() {
        let vec = vec![1u8, 2, 3, 4, 5];
        let body = to_boxed(vec.clone());
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected.as_ref(), vec.as_slice());
    }

    #[tokio::test]
    async fn test_boxed() {
        use http_body_util::Full;
        let full_body = Full::new(Bytes::from("test data"));
        let boxed_body: BoxBody = boxed(full_body);
        let collected = collect_bytes(boxed_body).await.unwrap();
        assert_eq!(collected, Bytes::from("test data"));
    }

    #[tokio::test]
    async fn test_boxed_sync() {
        use http_body_util::Full;
        let full_body = Full::new(Bytes::from("sync test"));
        let boxed_body: BoxBodySync = boxed_sync(full_body);
        let collected = collect_bytes(boxed_body).await.unwrap();
        assert_eq!(collected, Bytes::from("sync test"));
    }

    #[tokio::test]
    async fn test_wrap_stream_single_chunk() {
        use futures_util::stream;

        let data = Bytes::from("single chunk");
        let stream = stream::iter(vec![Ok::<_, std::io::Error>(data.clone())]);

        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, data);
    }

    #[tokio::test]
    async fn test_wrap_stream_multiple_chunks() {
        use futures_util::stream;

        let chunks = vec![
            Ok::<_, std::io::Error>(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
            Ok(Bytes::from("chunk3")),
        ];
        let expected = Bytes::from("chunk1chunk2chunk3");

        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, expected);
    }

    #[tokio::test]
    async fn test_wrap_stream_empty() {
        use futures_util::stream;

        let stream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::new())]);

        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected.len(), 0);
    }

    #[tokio::test]
    async fn test_wrap_stream_error() {
        use futures_util::stream;

        let chunks = vec![
            Ok::<_, std::io::Error>(Bytes::from("chunk1")),
            Err(std::io::Error::other("test error")),
        ];

        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let result = collect_bytes(body).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_wrap_stream_various_types() {
        use futures_util::stream;

        // Test that Into<Bytes> works for various types

        // Test with &str
        let chunks = vec![Ok::<_, std::io::Error>("string slice"), Ok("another string")];
        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("string sliceanother string"));

        // Test with String
        let chunks = vec![
            Ok::<_, std::io::Error>(String::from("owned ")),
            Ok(String::from("strings")),
        ];
        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("owned strings"));

        // Test with Vec<u8>
        let chunks = vec![
            Ok::<_, std::io::Error>(vec![72u8, 101, 108, 108, 111]), // "Hello"
            Ok(vec![32u8, 87, 111, 114, 108, 100]),                  // " World"
        ];
        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("Hello World"));

        // Test with &[u8]
        let chunks = vec![
            Ok::<_, std::io::Error>(&[98u8, 121, 116, 101] as &[u8]), // "byte"
            Ok(&[115u8, 33] as &[u8]),                                // "s!"
        ];
        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("bytes!"));

        // Test with custom struct implementing Into<Bytes>
        struct CustomChunk {
            data: String,
        }

        impl From<CustomChunk> for Bytes {
            fn from(chunk: CustomChunk) -> Bytes {
                Bytes::from(chunk.data)
            }
        }

        let chunks = vec![
            Ok::<_, std::io::Error>(CustomChunk { data: "custom ".into() }),
            Ok(CustomChunk { data: "struct".into() }),
        ];
        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("custom struct"));
    }

    #[tokio::test]
    async fn test_wrap_stream_custom_stream_type() {
        use bytes::Bytes;
        use std::pin::Pin;
        use std::task::{Context, Poll};

        // Custom stream type that implements futures_util::Stream
        struct CustomStream {
            chunks: Vec<Result<Bytes, std::io::Error>>,
        }

        impl CustomStream {
            fn new(chunks: Vec<Result<Bytes, std::io::Error>>) -> Self {
                Self { chunks }
            }
        }

        impl futures_util::Stream for CustomStream {
            type Item = Result<Bytes, std::io::Error>;

            fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                if self.chunks.is_empty() {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(self.chunks.remove(0)))
                }
            }
        }

        let stream = CustomStream::new(vec![Ok(Bytes::from("custom ")), Ok(Bytes::from("stream"))]);

        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("custom stream"));
    }

    #[tokio::test]
    async fn test_wrap_stream_custom_error_type() {
        use bytes::Bytes;
        use futures_util::stream;

        // Custom error type that implements Into<BoxError>
        #[derive(Debug, Clone)]
        struct CustomError {
            message: String,
        }

        impl std::fmt::Display for CustomError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "CustomError: {}", self.message)
            }
        }

        impl std::error::Error for CustomError {}

        // Test successful case with custom error type
        let chunks = vec![
            Ok::<_, CustomError>(Bytes::from("custom ")),
            Ok(Bytes::from("error type")),
        ];
        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("custom error type"));

        // Test error case with custom error type
        let chunks = vec![
            Ok::<_, CustomError>(Bytes::from("data")),
            Err(CustomError {
                message: "custom error".into(),
            }),
        ];
        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let result = collect_bytes(body).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_wrap_stream_incremental_consumption() {
        use bytes::Bytes;
        use http_body_util::BodyExt;
        use std::pin::Pin;
        use std::task::{Context, Poll};

        struct IncrementalStream {
            chunks: Vec<Result<Bytes, std::io::Error>>,
        }

        impl IncrementalStream {
            fn new(chunks: Vec<Result<Bytes, std::io::Error>>) -> Self {
                Self { chunks }
            }
        }

        impl futures_util::Stream for IncrementalStream {
            type Item = Result<Bytes, std::io::Error>;

            fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                if self.chunks.is_empty() {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(self.chunks.remove(0)))
                }
            }
        }

        let stream = IncrementalStream::new(vec![
            Ok(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
            Ok(Bytes::from("chunk3")),
        ]);

        let mut body = wrap_stream(stream);

        let frame1 = body.frame().await.unwrap().unwrap();
        assert!(frame1.is_data());
        assert_eq!(frame1.into_data().unwrap(), Bytes::from("chunk1"));

        let frame2 = body.frame().await.unwrap().unwrap();
        assert!(frame2.is_data());
        assert_eq!(frame2.into_data().unwrap(), Bytes::from("chunk2"));

        let frame3 = body.frame().await.unwrap().unwrap();
        assert!(frame3.is_data());
        assert_eq!(frame3.into_data().unwrap(), Bytes::from("chunk3"));

        let frame4 = body.frame().await;
        assert!(frame4.is_none());
    }

    #[tokio::test]
    async fn test_wrap_stream_sync_single_chunk() {
        use futures_util::stream;

        let data = Bytes::from("sync single chunk");
        let stream = stream::iter(vec![Ok::<_, std::io::Error>(data.clone())]);

        let body = wrap_stream_sync(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, data);
    }

    #[tokio::test]
    async fn test_wrap_stream_sync_multiple_chunks() {
        use futures_util::stream;

        let chunks = vec![
            Ok::<_, std::io::Error>(Bytes::from("sync1")),
            Ok(Bytes::from("sync2")),
            Ok(Bytes::from("sync3")),
        ];
        let expected = Bytes::from("sync1sync2sync3");

        let stream = stream::iter(chunks);
        let body = wrap_stream_sync(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, expected);
    }

    #[tokio::test]
    async fn test_empty_sync_body() {
        let body = empty_sync();
        let bytes = collect_bytes(body).await.unwrap();
        assert_eq!(bytes.len(), 0);
    }

    #[tokio::test]
    async fn test_to_boxed_sync() {
        let data = Bytes::from("sync boxed data");
        let body = to_boxed_sync(data.clone());
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, data);
    }

    // Compile-time tests to ensure Send/Sync bounds are correct
    // Following the pattern used by hyper and axum
    fn _assert_send<T: Send>() {}
    fn _assert_sync<T: Sync>() {}

    fn _assert_send_sync_bounds() {
        // BoxBodySync must be both Send and Sync
        _assert_send::<BoxBodySync>();
        _assert_sync::<BoxBodySync>();

        // BoxBody must be Send (but is intentionally NOT Sync - it's UnsyncBoxBody)
        _assert_send::<BoxBody>();
    }

    #[tokio::test]
    async fn test_wrap_stream_sync_produces_sync_body() {
        use futures_util::stream;

        let data = Bytes::from("test sync");
        let stream = stream::iter(vec![Ok::<_, std::io::Error>(data.clone())]);

        let body = wrap_stream_sync(stream);

        // Compile-time check: ensure the body is Sync
        fn check_sync<T: Sync>(_: &T) {}
        check_sync(&body);

        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, data);
    }

    #[test]
    fn test_empty_sync_is_sync() {
        let body = empty_sync();
        fn check_sync<T: Sync>(_: &T) {}
        check_sync(&body);
    }

    #[test]
    fn test_boxed_sync_is_sync() {
        use http_body_util::Full;
        let body = boxed_sync(Full::new(Bytes::from("test")));
        fn check_sync<T: Sync>(_: &T) {}
        check_sync(&body);
    }

    #[tokio::test]
    async fn collection_preserves_trailers_with_and_without_a_limit() {
        for limit in [0, 1024] {
            let mut trailers = http::HeaderMap::new();
            trailers.insert("checksum", http::HeaderValue::from_static("abc"));
            let frames = vec![
                Ok::<_, std::convert::Infallible>(http_body::Frame::data(Bytes::from_static(b"payload"))),
                Ok(http_body::Frame::trailers(trailers.clone())),
            ];
            let body = http_body_util::StreamBody::new(futures_util::stream::iter(frames));
            let (bytes, actual_trailers) = collect_body_with_trailers(
                body,
                Some(
                    CollectionConfig::builder()
                        .maybe_limit(NonZeroUsize::new(limit))
                        .build(),
                ),
            )
            .await
            .unwrap();
            assert_eq!(bytes, "payload");
            assert_eq!(actual_trailers, Some(trailers));
        }
    }

    #[tokio::test]
    async fn collection_handles_huge_size_hints_without_huge_reservations() {
        struct FailingBody<'a> {
            lower: u64,
            polled: &'a mut bool,
        }

        impl HttpBody for FailingBody<'_> {
            type Data = Bytes;
            type Error = std::io::Error;

            fn poll_frame(
                mut self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
                *self.polled = true;
                std::task::Poll::Ready(Some(Err(std::io::Error::other("body read failed"))))
            }

            fn size_hint(&self) -> http_body::SizeHint {
                http_body::SizeHint::with_exact(self.lower)
            }
        }

        // Both an unlimited body and a large allowed body must reach the read error
        // without attempting to allocate the advertised size first.
        for limit in [0, usize::MAX] {
            let mut polled = false;
            let body = FailingBody {
                lower: usize::MAX as u64,
                polled: &mut polled,
            };
            assert!(matches!(
                collect_body(
                    body,
                    Some(
                        CollectionConfig::builder()
                            .maybe_limit(NonZeroUsize::new(limit))
                            .build()
                    )
                )
                .await,
                Err(CollectBodyError::Body(_))
            ));
            assert!(polled);
        }

        // Check the full hint before narrowing to usize or applying the reservation cap.
        let mut polled = false;
        let body = FailingBody {
            lower: u64::MAX,
            polled: &mut polled,
        };
        assert!(matches!(
            collect_body(body, Some(CollectionConfig::builder().maybe_limit(NonZeroUsize::new(1024)).build())).await,
            Err(CollectBodyError::BodyLimitExceeded(limit)) if limit.get() == 1024
        ));
        assert!(!polled);
    }

    #[tokio::test]
    async fn collection_grows_beyond_initial_reservation() {
        let bytes = Bytes::from(vec![b'x'; 16 * 1024]);
        for limit in [0, bytes.len()] {
            let body = http_body_util::Full::new(bytes.clone());
            assert_eq!(
                collect_body(
                    body,
                    Some(
                        CollectionConfig::builder()
                            .maybe_limit(NonZeroUsize::new(limit))
                            .build()
                    )
                )
                .await
                .unwrap(),
                bytes
            );
        }
    }

    #[tokio::test]
    async fn test_collect_body_limited_under_limit() {
        let body = from_bytes(Bytes::from("hello"));
        let result = collect_body(
            body,
            Some(CollectionConfig::builder().maybe_limit(NonZeroUsize::new(1024)).build()),
        )
        .await;
        assert_eq!(result.unwrap(), Bytes::from("hello"));
    }

    #[tokio::test]
    async fn test_collect_body_limited_over_limit() {
        let body = from_bytes(Bytes::from("this is way too long"));
        let result = collect_body(
            body,
            Some(CollectionConfig::builder().maybe_limit(NonZeroUsize::new(5)).build()),
        )
        .await;
        assert!(matches!(
            result,
            Err(CollectBodyError::BodyLimitExceeded(limit)) if limit.get() == 5
        ));
    }

    #[tokio::test]
    async fn test_collect_body_limited_chunked_exceeds_mid_stream() {
        use futures_util::stream;

        // Simulate a chunked body where the limit is crossed on the second chunk.
        let chunks = vec![
            Ok::<_, std::io::Error>(http_body::Frame::data(Bytes::from("aaaa"))), // 4 bytes
            Ok(http_body::Frame::data(Bytes::from("bbbb"))),                      // +4 = 8, over limit of 6
        ];
        let body = http_body_util::StreamBody::new(stream::iter(chunks));
        let result = collect_body(
            body,
            Some(CollectionConfig::builder().maybe_limit(NonZeroUsize::new(6)).build()),
        )
        .await;
        assert!(matches!(result, Err(CollectBodyError::BodyLimitExceeded(_))));
    }

    #[tokio::test]
    async fn test_collect_body_limited_empty_body() {
        let body = empty();
        let result = collect_body(
            body,
            Some(CollectionConfig::builder().maybe_limit(NonZeroUsize::new(100)).build()),
        )
        .await;
        assert_eq!(result.unwrap(), Bytes::from(""));
    }
}
