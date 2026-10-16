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
mod tests;
