/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! HTTP server runtime and utilities, loosely based on Axum.

#[macro_use]
pub(crate) mod macros;

pub mod body;
mod clone_box_service;
pub mod error;
mod extension;

// Only the code-generated operation registry should instantiate routers.
// We therefore hide it in the documentation.
#[doc(hidden)]
pub mod routing;

#[doc(hidden)]
pub mod protocols;
pub mod rejection;

#[doc(inline)]
pub use self::body::{Body, BoxBody, HttpBody};
#[doc(inline)]
pub use self::error::Error;
#[doc(inline)]
pub use self::extension::Extension;
#[doc(inline)]
pub use self::routing::Router;
#[doc(inline)]
pub use tower_http::add_extension::{AddExtension, AddExtensionLayer};

/// Alias for a type-erased error type.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(test)]
mod test_helpers;
