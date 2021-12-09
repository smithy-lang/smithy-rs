/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP body utilities.
#[doc(no_inline)]
pub use http_body::Body as HttpBody;

#[doc(no_inline)]
pub use hyper::body::Body;

#[doc(no_inline)]
pub use axum_core::body::{boxed, BoxBody};

pub(crate) fn empty() -> BoxBody {
    boxed(http_body::Empty::new())
}

/// Convert a generic [`Body`] into a [`BoxBody`]. This is used by the codegen to
/// simplify the generation logic.
pub fn to_boxed<B>(body: B) -> BoxBody
where
    Body: From<B>,
{
    boxed(Body::from(body))
}
