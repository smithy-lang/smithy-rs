/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! High-level DynamoDB mapper for the AWS SDK for Rust.
//!
//! This crate provides a type-safe, ergonomic interface for working with
//! DynamoDB tables. It supports derive macros for automatic schema generation,
//! type-safe expressions, and efficient operations.
//!
//! # Example
//!
//! ```ignore
//! use aws_sdk_dynamodb_mapper::DdbItem;
//!
//! #[derive(DdbItem)]
//! struct User {
//!     #[partition_key]
//!     id: String,
//!     name: String,
//!     email: String,
//! }
//! ```

#![warn(missing_docs)]

pub use aws_sdk_dynamodb_mapper_core::*;
pub use aws_sdk_dynamodb_mapper_macros::*;

/// Expression builders for DynamoDB condition, update, and projection expressions.
pub mod expressions {
    pub use aws_sdk_dynamodb_expressions::*;
}
