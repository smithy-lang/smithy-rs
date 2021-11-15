/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP server runtime and utilities, loosely based on Axum.

#![cfg_attr(docsrs, feature(doc_cfg))]

#[macro_use]
pub(crate) mod macros;

mod body;
mod error;

pub mod protocols;
pub mod rejection;

#[doc(inline)]
pub use self::body::{box_body, Body, BoxBody, HttpBody};
#[doc(inline)]
pub use self::error::Error;

/// Alias for a type-erased error type.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
