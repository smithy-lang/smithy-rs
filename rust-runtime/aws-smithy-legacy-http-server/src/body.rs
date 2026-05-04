/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP body utilities.

// Used in the codegen in trait bounds.
#[doc(hidden)]
pub use http_body::Body as HttpBody;

pub use hyper::body::Body;

use bytes::Bytes;

use crate::error::{BoxError, Error};

/// The primary [`Body`] returned by the generated `smithy-rs` service.
pub type BoxBody = http_body::combinators::UnsyncBoxBody<Bytes, Error>;

// `boxed` is used in the codegen of the implementation of the operation `Handler` trait.
/// Convert a [`http_body::Body`] into a [`BoxBody`].
pub fn boxed<B>(body: B) -> BoxBody
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    try_downcast(body).unwrap_or_else(|body| body.map_err(Error::new).boxed_unsync())
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

pub(crate) fn empty() -> BoxBody {
    boxed(http_body::Empty::new())
}

/// Convert anything that can be converted into a [`hyper::body::Body`] into a [`BoxBody`].
/// This simplifies codegen a little bit.
#[doc(hidden)]
pub fn to_boxed<B>(body: B) -> BoxBody
where
    Body: From<B>,
{
    boxed(Body::from(body))
}

// ============================================================================
// Size-limited Body Collection
// ============================================================================

use std::fmt;
use std::future::poll_fn;
use std::pin::Pin;

/// An error produced by [`collect_body_limited`] when the body exceeds the
/// configured limit.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct BodyLimitExceeded {
    /// The configured maximum, in bytes.
    pub limit: usize,
}

impl fmt::Display for BodyLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "request body exceeded the configured maximum of {} bytes",
            self.limit
        )
    }
}

impl std::error::Error for BodyLimitExceeded {}

/// The error returned by [`collect_body_limited`].
///
/// Either the underlying body produced an error, or the configured size limit was
/// exceeded. The generic over `E` lets the helper work with body error types that
/// are `Send` but not `Sync` (the generated-server code carries only a `Send` bound).
#[doc(hidden)]
#[derive(Debug)]
pub enum CollectBodyError<E> {
    /// The underlying body produced an error while being read.
    Body(E),
    /// The body exceeded the configured maximum size.
    TooLarge(BodyLimitExceeded),
}

impl<E: fmt::Display> fmt::Display for CollectBodyError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Body(e) => write!(f, "error reading request body: {e}"),
            Self::TooLarge(e) => e.fmt(f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CollectBodyError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Body(e) => Some(e),
            Self::TooLarge(e) => Some(e),
        }
    }
}

/// Collect an HTTP body into a [`Bytes`] buffer, enforcing a maximum size.
///
/// If the body produces more than `limit` bytes the function returns an error
/// *before* buffering additional data. This bounds server memory regardless of
/// what the client sends (for example, via `Transfer-Encoding: chunked`).
///
/// Passing `limit == 0` disables the check and collects the entire body (the
/// historical behavior, *not* recommended — see the security notes on the
/// `requestBodyMaxBytes` codegen setting).
#[doc(hidden)]
pub async fn collect_body_limited<B>(body: B, limit: usize) -> Result<Bytes, CollectBodyError<B::Error>>
where
    B: HttpBody,
{
    // `http-body` 0.4's core method `poll_data` requires a `Pin<&mut Self>`. Heap-pin
    // so this works for any `B: HttpBody` without requiring `Unpin`.
    let mut body: Pin<Box<B>> = Box::pin(body);
    let mut buf: Vec<u8> = Vec::new();

    loop {
        let chunk_opt = poll_fn(|cx| body.as_mut().poll_data(cx)).await;
        match chunk_opt {
            None => break,
            Some(Err(e)) => return Err(CollectBodyError::Body(e)),
            Some(Ok(mut chunk)) => {
                use bytes::Buf;
                let chunk_len = chunk.remaining();
                if limit > 0 && buf.len().saturating_add(chunk_len) > limit {
                    return Err(CollectBodyError::TooLarge(BodyLimitExceeded { limit }));
                }
                buf.reserve(chunk_len);
                while chunk.has_remaining() {
                    let slice = chunk.chunk();
                    let len = slice.len();
                    buf.extend_from_slice(slice);
                    chunk.advance(len);
                }
            }
        }
    }

    Ok(Bytes::from(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_collect_body_limited_under_limit() {
        let body = Body::from("hello");
        let result = collect_body_limited(body, 1024).await;
        assert_eq!(result.unwrap(), Bytes::from("hello"));
    }

    #[tokio::test]
    async fn test_collect_body_limited_over_limit() {
        let body = Body::from("this is way too long");
        let result = collect_body_limited(body, 5).await;
        assert!(matches!(
            result,
            Err(CollectBodyError::TooLarge(BodyLimitExceeded { limit: 5 }))
        ));
    }

    #[tokio::test]
    async fn test_collect_body_limited_chunked_exceeds_mid_stream() {
        use futures_util::stream;

        // Simulate a chunked body where the limit is crossed on the second chunk.
        let chunks: Vec<Result<&[u8], std::io::Error>> = vec![
            Ok(b"aaaa"), // 4 bytes
            Ok(b"bbbb"), // +4 = 8, over limit of 6
        ];
        let body = Body::wrap_stream(stream::iter(chunks));
        let result = collect_body_limited(body, 6).await;
        assert!(matches!(result, Err(CollectBodyError::TooLarge(_))));
    }

    #[tokio::test]
    async fn test_collect_body_limited_empty_body() {
        let body = Body::empty();
        let result = collect_body_limited(body, 100).await;
        assert_eq!(result.unwrap(), Bytes::from(""));
    }
}
