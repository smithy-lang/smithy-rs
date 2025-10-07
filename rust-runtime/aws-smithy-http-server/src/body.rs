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
    boxed(imports::HyperBody::from(bytes))
}

#[cfg(feature = "http-1x")]
pub fn from_bytes(bytes: Bytes) -> BoxBody {
    use imports::Full;
    boxed(Full::new(bytes))
}
