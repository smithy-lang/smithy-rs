/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Core traits and types for the AWS SDK DynamoDB Mapper.
//!
//! This crate provides the foundational traits used by the DynamoDB Mapper library:
//!
//! - [`AttributeValueConvert`] - Convert individual values to/from DynamoDB AttributeValues
//! - [`ItemConverter`] - Convert complete structs to/from DynamoDB items
//! - [`ItemSchema`] - Define key structure for tables and indexes
//! - [`ProvideItemSchema`] - Link types to their schema implementations
//! - [`ExtractKey`] - Efficiently extract keys from objects
//!
//! # Example
//!
//! ```ignore
//! use aws_sdk_dynamodb_mapper_core::{
//!     AttributeValueConvert, ItemConverter, ItemSchema, ProvideItemSchema, NoSortKey,
//! };
//!
//! struct User {
//!     id: String,
//!     name: String,
//! }
//!
//! struct UserSchema;
//!
//! impl ItemSchema<User> for UserSchema {
//!     type PartitionKey = String;
//!     type SortKey = NoSortKey;
//!
//!     fn partition_key_names(&self) -> &'static [&'static str] { &["id"] }
//!     fn sort_key_names(&self) -> Option<&'static [&'static str]> { None }
//! }
//! ```

#![warn(missing_docs)]

mod convert;
pub mod error;
pub(crate) mod key;
mod traits;

pub use error::{ConversionError, ConversionErrorKind};
pub use traits::{
    AttributeValueConvert, ExtractKey, ItemConverter, ItemSchema, NoSortKey, ProvideItemSchema,
};
