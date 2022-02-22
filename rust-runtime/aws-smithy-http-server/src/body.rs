/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP body utilities.
#[doc(hidden)]
pub use http_body::Body as HttpBody;

#[doc(hidden)]
pub use hyper::body::Body;

// `boxed` is used in the codegen of the implementation of the operation `Handler` trait.
#[doc(hidden)]
pub use axum_core::body::{boxed, BoxBody};

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
