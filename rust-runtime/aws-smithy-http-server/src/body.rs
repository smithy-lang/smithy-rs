/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP body utilities.

// This file is a copy of Axum's `body.rs` (https://github.com/tokio-rs/axum/blob/2507463706d0cea90007b5959c579a32d4b24cc4/axum/src/body/mod.rs#L1)
// Axum's original license can be found inside the file named `LICENSE.mit`.
use crate::BoxError;
use crate::Error;

#[doc(no_inline)]
pub use http_body::{Body as HttpBody, Empty, Full};

#[doc(no_inline)]
pub use hyper::body::Body;

#[doc(no_inline)]
pub use bytes::Bytes;

/// A boxed [`Body`] trait object.
///
/// This is used as the response body type for applications. Its
/// necessary to unify multiple response bodies types into one.
pub type BoxBody = http_body::combinators::UnsyncBoxBody<Bytes, Error>;

/// Convert a [`http_body::Body`] into a [`BoxBody`].
pub fn box_body<B>(body: B) -> BoxBody
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    body.map_err(Error::new).boxed_unsync()
}
