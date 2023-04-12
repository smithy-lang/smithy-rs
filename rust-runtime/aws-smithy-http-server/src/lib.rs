/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg_attr(docsrs, feature(doc_cfg))]

//! HTTP server runtime and utilities, loosely based on [axum].
//!
//! [axum]: https://docs.rs/axum/latest/axum/
#[macro_use]
pub(crate) mod macros;

pub mod body;
pub(crate) mod error;
pub mod extension;
pub mod instrumentation;
pub mod operation;
pub mod plugin;
#[doc(hidden)]
pub mod protocols;
#[doc(hidden)]
pub mod rejection;
pub mod request;
#[doc(hidden)]
pub mod response;
pub mod routing;
#[doc(hidden)]
pub mod runtime_error;

#[doc(inline)]
pub(crate) use self::error::Error;
#[doc(inline)]
pub use self::request::extension::Extension;
#[doc(inline)]
pub use tower_http::add_extension::{AddExtension, AddExtensionLayer};

#[cfg(test)]
mod test_helpers;

#[doc(hidden)]
pub mod proto;
