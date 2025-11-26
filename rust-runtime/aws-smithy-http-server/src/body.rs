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

// Used in the codegen in trait bounds.
#[doc(hidden)]
pub use http_body::Body as HttpBody;

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

#[doc(hidden)]
pub(crate) fn try_downcast<T, K>(k: K) -> Result<T, K>
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

/// Convert bytes or similar types into a [`BoxBody`].
///
/// This simplifies codegen a little bit.
#[doc(hidden)]
pub fn to_boxed<B>(body: B) -> BoxBody
where
    B: Into<Bytes>,
{
    boxed(http_body_util::Full::new(body.into()))
}

/// Convert bytes or similar types into a [`BoxBodySync`].
///
/// This simplifies codegen a little bit.
#[doc(hidden)]
pub fn to_boxed_sync<B>(body: B) -> BoxBodySync
where
    B: Into<Bytes>,
{
    boxed_sync(http_body_util::Full::new(body.into()))
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
mod tests {
    use super::*;

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
        let chunks = vec![Ok::<_, std::io::Error>("string slice"), Ok("another string")];

        let stream = stream::iter(chunks);
        let body = wrap_stream(stream);
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, Bytes::from("string sliceanother string"));
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
}
