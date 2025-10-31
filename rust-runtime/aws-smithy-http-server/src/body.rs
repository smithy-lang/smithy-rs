/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP body utilities.
//!
//! This module provides a stable API for body handling regardless of the
//! underlying HTTP version. The implementation uses conditional compilation
//! to select the appropriate types and functions based on the `http-1x` feature.

use crate::error::{BoxError, Error};
use bytes::Bytes;

pub(crate) use http_body_util::{BodyExt, Empty, Full};

// Used in the codegen in trait bounds.
#[doc(hidden)]
pub use http_body::Body as HttpBody;

// ============================================================================
// BoxBody - Type-Erased Body
// ============================================================================

/// The primary body type returned by the generated `smithy-rs` service.
///
/// This provides a stable public API regardless of HTTP version.
/// Internally it uses `UnsyncBoxBody` from the appropriate http-body version.
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
    boxed(Empty::<Bytes>::new())
}

/// Create an empty sync body.
pub fn empty_sync() -> BoxBodySync {
    boxed_sync(Empty::<Bytes>::new())
}

/// Convert bytes or similar types into a [`BoxBody`] for HTTP 1.x.
#[doc(hidden)]
pub fn to_boxed<B>(body: B) -> BoxBody
where
    B: Into<Bytes>,
{
    boxed(Full::new(body.into()))
}

/// Convert bytes or similar types into a [`BoxBodySync`] for HTTP 1.x.
#[doc(hidden)]
pub fn to_boxed_sync<B>(body: B) -> BoxBodySync
where
    B: Into<Bytes>,
{
    boxed_sync(Full::new(body.into()))
}

// ============================================================================
// Body Reading Functions
// ============================================================================

/// Collect all bytes from a body.
///
/// This provides a version-agnostic way to read body contents.
/// In HTTP 0.x, this uses `hyper::body::to_bytes()`.
/// In HTTP 1.x, this uses `BodyExt::collect()`.
pub async fn collect_bytes<B>(body: B) -> Result<Bytes, Error>
where
    B: HttpBody,
    B::Error: Into<BoxError>,
{
    use http_body_util::BodyExt;

    let collected = body.collect().await.map_err(Error::new)?;
    Ok(collected.to_bytes())
}

/// Create a body from bytes.
pub fn from_bytes(bytes: Bytes) -> BoxBody {
    boxed(Full::new(bytes))
}

// ============================================================================
// Stream Wrapping for Event Streaming
// ============================================================================

/// Wrap a stream of byte chunks into a BoxBody for HTTP 1.x.
///
/// This is used for event streaming support. The stream should produce `Result<O, E>`
/// where `O` can be converted into `Bytes` and `E` can be converted into an error.
///
/// For HTTP 0.x, this is not needed since `Body::wrap_stream` exists on hyper::Body.
/// For HTTP 1.x, we provide this as a module-level function since `Body` is just a type alias
/// for `hyper::body::Incoming` which doesn't have a `wrap_stream` method.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
