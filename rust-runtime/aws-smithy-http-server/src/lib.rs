/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![allow(clippy::derive_partial_eq_without_eq)]

//! HTTP server runtime and utilities, loosely based on [axum].
//!
//! [axum]: https://docs.rs/axum/latest/axum/
#[macro_use]
pub(crate) mod macros;

pub mod body;
pub(crate) mod error;
pub mod extension;
pub mod instrumentation;
pub mod layer;
pub mod operation;
pub mod plugin;
#[doc(hidden)]
pub mod protocol;
#[doc(hidden)]
pub mod rejection;
pub mod request;
#[doc(hidden)]
pub mod response;
pub mod routing;
#[doc(hidden)]
pub mod runtime_error;
pub mod service;
pub mod shape_id;

#[doc(inline)]
pub(crate) use self::error::Error;
#[doc(inline)]
pub use self::request::extension::Extension;
#[doc(inline)]
pub use tower_http::add_extension::{AddExtension, AddExtensionLayer};

#[cfg(test)]
mod test_helpers;

#[cfg(not(feature = "http-1x"))]
pub use http_02x as http;

#[cfg(feature = "http-1x")]
pub use http_1x as http;

#[cfg(not(feature = "http-1x"))]
pub(crate) use aws_smithy_http;

#[cfg(feature = "http-1x")]
pub(crate) use aws_smithy_http_1x as aws_smithy_http;

// Re-export the appropriate version of aws-smithy-json
#[cfg(not(feature = "http-1x"))]
pub(crate) use aws_smithy_json_02x as aws_smithy_json;

#[cfg(feature = "http-1x")]
pub(crate) use aws_smithy_json_1x as aws_smithy_json;
