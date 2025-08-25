/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP 1.x body utilities.
//!
//! This module provides body types and utilities for HTTP 1.x support,
//! following a similar pattern to Axum's body wrapper approach.

use bytes::Bytes;
pub use http_body_1x::Body as HttpBodyTrait;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::BodyExt;

use crate::error::{BoxError, Error};

// fzq: why do we use unsyncboxbody? i need a solid example to understand this
// how do we handle streaming cases? do we need this to be an enum?
// trialers? and then empty body that the article talks about?
// do we need   - Bidirectional adaptation: HTTP 1.x ↔ HTTP 0.4.x conversion utilities, like SDKBody
// why are we not using SdkBody?

/// The primary [`Body`] returned by the generated `smithy-rs` service for HTTP 1.x.
///
/// This is a type-erased body that can wrap any body implementing [`http_body_1x::Body`].
/// It's designed to provide a stable, ergonomic API similar to Axum's approach.
pub struct Body(UnsyncBoxBody<Bytes, Error>);

impl Body {
    /// Create a new body from any type that implements [`http_body_1x::Body`].
    pub fn new<B, E>(body: B) -> Self
    where
        B: HttpBodyTrait<Data = Bytes, Error = E> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        Self(body.map_err(Into::into).boxed_unsync())
    }

    /// Create an empty body.
    pub fn empty() -> Self {
        Self::new(http_body_util::Empty::new())
    }

    /// Create a body from static bytes.
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self::new(http_body_util::Full::new(bytes))
    }

    /// Create a body from a static string.
    pub fn from_str(s: &str) -> Self {
        Self::from_bytes(Bytes::copy_from_slice(s.as_bytes()))
    }

    // TODO: more functions to follow as axum has done, for example,
    // for vec<u8> etc.
}

// Implement the http-body trait for our Body wrapper
impl HttpBodyTrait for Body {
    type Data = Bytes;
    type Error = Error;

    // fzq: i know why you pin stuff and this is how the trait has been defined but
    // what is that pin::Pin::new stuff? does axum do this? if yes, why?
    // oh pin the inner and then call the method on it is the reason.
    // have we done anything like that before? has axum done any thing like this?
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
        std::pin::Pin::new(&mut self.0).poll_frame(cx)
    }

    fn size_hint(&self) -> http_body_1x::SizeHint {
        self.0.size_hint()
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }
}

// fzq: why not pub type BoxBody = Box<Body>;
// Convenience type alias that matches the pattern in body.rs
pub type BoxBody = Body;

// Re-export the http-body trait for convenience
// pub use http_body_1x::Body as HttpBody;

/// Convert a [`http_body_1x::Body`] into a [`Body`].
/// This is the HTTP 1.x equivalent of the `boxed` function in body.rs.
pub fn boxed<B>(body: B) -> Body
where
    B: HttpBodyTrait<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    Body::new(body)
}

/// Create an empty body.
/// This matches the API from body.rs for consistency.
pub fn empty() -> Body {
    Body::empty()
}

/// Convert anything that can be converted into a [`Body`] into a [`Body`].
/// This simplifies codegen a little bit, matching the API from body.rs.
pub fn to_boxed<T>(value: T) -> Body
where
    Body: From<T>,
{
    Body::from(value)
}

// Common From implementations for ergonomics
impl From<&'static str> for Body {
    fn from(s: &'static str) -> Self {
        Body::from_str(s)
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        Body::from_bytes(Bytes::from(s))
    }
}

impl From<&'static [u8]> for Body {
    fn from(bytes: &'static [u8]) -> Self {
        Body::from_bytes(Bytes::from_static(bytes))
    }
}

impl From<Vec<u8>> for Body {
    fn from(bytes: Vec<u8>) -> Self {
        Body::from_bytes(Bytes::from(bytes))
    }
}

impl From<Bytes> for Body {
    fn from(bytes: Bytes) -> Self {
        Body::from_bytes(bytes)
    }
}

// many more to follow based on what axum has done.
