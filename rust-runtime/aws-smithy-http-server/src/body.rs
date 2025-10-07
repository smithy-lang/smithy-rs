/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP body utilities.

// Used in the codegen in trait bounds.
#[doc(hidden)]
#[cfg(not(feature = "http-1x"))]
pub use http_body_04x::Body as HttpBody;

#[doc(hidden)]
#[cfg(feature = "http-1x")]
pub use http_body_1x::Body as HttpBody;

// Public re-export of Body type for backward compatibility
#[cfg(not(feature = "http-1x"))]
pub use hyper_014::body::Body;

#[cfg(not(feature = "http-1x"))]
mod imports {
    pub use hyper_014::body::Body as HyperBody;
}

#[cfg(feature = "http-1x")]
mod imports {
    pub use http_body_util::{BodyExt, Empty, Full};
}

// For http-1x, hyper::Body doesn't exist. We export hyper::body::Incoming as Body to make it easier
// port middleware that uses `Request<Body>`.
#[cfg(feature = "http-1x")]
pub use hyper_1x::body::Incoming as Body;

use bytes::Bytes;

use crate::error::{BoxError, Error};

/// The primary [`Body`] returned by the generated `smithy-rs` service.
#[cfg(not(feature = "http-1x"))]
pub type BoxBody = http_body_04x::combinators::UnsyncBoxBody<Bytes, Error>;

#[cfg(feature = "http-1x")]
pub type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, Error>;

// `boxed` is used in the codegen of the implementation of the operation `Handler` trait.
/// Convert an HTTP body implementing [`http_body::Body`](http_body_04x::Body) into a [`BoxBody`].
#[cfg(not(feature = "http-1x"))]
#[cfg_attr(docsrs, doc(cfg(not(feature = "http-1x"))))]
pub fn boxed<B>(body: B) -> BoxBody
where
    B: http_body_04x::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    try_downcast(body).unwrap_or_else(|body| {
        use http_body_04x::Body as _;
        body.map_err(Error::new).boxed_unsync()
    })
}

/// Convert an HTTP body implementing [`http_body::Body`](http_body_1x::Body) into a [`BoxBody`].
#[cfg(feature = "http-1x")]
#[cfg_attr(docsrs, doc(cfg(feature = "http-1x")))]
pub fn boxed<B>(body: B) -> BoxBody
where
    B: http_body_1x::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    use imports::BodyExt;
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

/// Create an empty body.
#[cfg(not(feature = "http-1x"))]
pub fn empty() -> BoxBody {
    boxed(http_body_04x::Empty::new())
}

#[cfg(feature = "http-1x")]
pub fn empty() -> BoxBody {
    boxed(http_body_util::Empty::<Bytes>::new())
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

/// Convert bytes or similar types into a [`BoxBody`] for HTTP 1.x.
#[doc(hidden)]
#[cfg(feature = "http-1x")]
pub fn to_boxed<B>(body: B) -> BoxBody
where
    B: Into<Bytes>,
{
    boxed(http_body_util::Full::new(body.into()))
}

// ============================================================================
// Body Reading Functions
// ============================================================================

/// Collect all bytes from a body.
///
/// This provides a version-agnostic way to read body contents.
/// In HTTP 0.x, this uses `hyper::body::to_bytes()`.
/// In HTTP 1.x, this uses `BodyExt::collect()`.
#[cfg(not(feature = "http-1x"))]
pub async fn collect_bytes<B>(body: B) -> Result<Bytes, Error>
where
    B: HttpBody,
    B::Error: Into<BoxError>,
{
    hyper_014::body::to_bytes(body).await.map_err(|e| Error::new(e))
}

#[cfg(feature = "http-1x")]
pub async fn collect_bytes<B>(body: B) -> Result<Bytes, Error>
where
    // TODO-HTTP1x: do we need to allow anything that can be converted into HttpBody?
    B: HttpBody,
    B::Error: Into<BoxError>,
{
    use http_body_util::BodyExt;
    let collected = body.collect().await.map_err(|e| Error::new(e))?;
    Ok(collected.to_bytes())
}

/// Create a body from bytes.
#[cfg(not(feature = "http-1x"))]
pub fn from_bytes(bytes: Bytes) -> BoxBody {
    boxed(hyper_014::body::Body::from(bytes))
}

#[cfg(feature = "http-1x")]
pub fn from_bytes(bytes: Bytes) -> BoxBody {
    boxed(hyper_body_util::Full::new(bytes))
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
        let data = Bytes::from("Sample body");
        let body = from_bytes(data.clone());
        let collected = collect_bytes(body).await.unwrap();
        assert_eq!(collected, data);
    }

    #[tokio::test]
    async fn test_to_boxed_string() {
        let s = "Sample body";
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
}
