/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A [`ShapeId`] represents a [Smithy Shape ID](https://smithy.io/2.0/spec/model.html#shape-id).
//!
//! # Example
//!
//! In the following model:
//!
//! ```smithy
//! namespace smithy.example
//!
//! operation CheckHealth {}
//! ```
//!
//! - `absolute` is `"smithy.example#CheckHealth"`
//! - `namespace` is `"smithy.example"`
//! - `name` is `"CheckHealth"`

pub use crate::request::extension::{Extension, MissingExtension};

/// Represents a [Smithy Shape ID](https://smithy.io/2.0/spec/model.html#shape-id).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeId {
    absolute: &'static str,

    namespace: &'static str,
    name: &'static str,
}

impl ShapeId {
    /// Constructs a new [`ShapeId`]. This is used by the code-generator which preserves the invariants of the Shape ID format.
    #[doc(hidden)]
    pub const fn new(absolute: &'static str, namespace: &'static str, name: &'static str) -> Self {
        Self {
            absolute,
            namespace,
            name,
        }
    }

    /// Returns the namespace.
    ///
    /// See [Shape ID](https://smithy.io/2.0/spec/model.html#shape-id) for a breakdown of the syntax.
    pub fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// Returns the member name.
    ///
    /// See [Shape ID](https://smithy.io/2.0/spec/model.html#shape-id) for a breakdown of the syntax.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the absolute shape ID.
    ///
    /// See [Shape ID](https://smithy.io/2.0/spec/model.html#shape-id) for a breakdown of the syntax.
    pub fn absolute(&self) -> &'static str {
        self.absolute
    }
}
