/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP server runtime and utilities, loosely based on [axum].
//!
//! [axum]: https://docs.rs/axum/latest/axum/

#[macro_use]
pub(crate) mod macros;

pub mod body;
pub mod error;
mod extension;
pub mod routing;

#[doc(hidden)]
pub mod protocols;
pub mod rejection;

#[doc(inline)]
pub use self::body::{boxed, to_boxed, Body, BoxBody, HttpBody};
#[doc(inline)]
pub use self::error::Error;
#[doc(inline)]
pub use self::extension::{Extension, ExtensionModeledError, ExtensionRejection, ResponseExtensions};
#[doc(inline)]
pub use self::routing::Router;
#[doc(inline)]
pub use tower_http::add_extension::{AddExtension, AddExtensionLayer};

/// Alias for a type-erased error type.
pub use axum_core::BoxError;

#[cfg(test)]
mod test_helpers;
