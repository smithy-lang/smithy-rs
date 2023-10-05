/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol-agnostic types for smithy-rs.

#![allow(clippy::derive_partial_eq_without_eq)]
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]
pub mod base64;
pub mod body;
pub mod byte_stream;
/// A typemap for storing configuration.
pub mod config_bag;
pub mod date_time;
pub mod endpoint;
pub mod error;
// Marked as `doc(hidden)` because a type in the module is used both by this crate and by the code
// generator, but not by external users. Also, by the module being `doc(hidden)` instead of it being
// in `rust-runtime/inlineable`, each user won't have to pay the cost of running the module's tests
// when compiling their generated SDK.
#[doc(hidden)]
pub mod futures_stream_adapter;
pub mod primitive;
pub mod retry;
pub mod timeout;

/// Utilities for type erasure.
pub mod type_erasure;

mod blob;
mod document;
mod number;

pub use blob::Blob;
pub use date_time::DateTime;
pub use document::Document;
// TODO(deprecated): Remove deprecated re-export
/// Use [error::ErrorMetadata] instead.
#[deprecated(
    note = "`aws_smithy_types::Error` has been renamed to `aws_smithy_types::error::ErrorMetadata`"
)]
pub use error::ErrorMetadata as Error;
pub use number::Number;
