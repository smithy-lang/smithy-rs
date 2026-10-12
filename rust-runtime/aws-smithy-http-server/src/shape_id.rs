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

    /// Converts a schema shape ID with static string components into a server shape ID.
    ///
    /// The strings are shared without allocating. The schema ID itself need not be borrowed
    /// for `'static`, but its string components must be static because the server ID stores them.
    ///
    /// ```
    /// use aws_smithy_http_server::ShapeId;
    ///
    /// const SCHEMA_ID: aws_smithy_schema::ShapeId<'static> =
    ///     aws_smithy_schema::shape_id!("example", "Operation");
    /// const ID: ShapeId = ShapeId::from_schema(&SCHEMA_ID);
    ///
    /// assert_eq!(ID.absolute(), SCHEMA_ID.as_str());
    /// ```
    pub const fn from_schema(id: &aws_smithy_schema::ShapeId<'static>) -> Self {
        Self::new(id.as_str(), id.namespace(), id.shape_name())
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

impl From<&aws_smithy_schema::ShapeId<'static>> for ShapeId {
    fn from(id: &aws_smithy_schema::ShapeId<'static>) -> Self {
        Self::from_schema(id)
    }
}

#[cfg(test)]
mod tests {
    use crate::ShapeId;

    #[test]
    fn legacy_shape_id_api_is_available_at_the_crate_root() {
        const ID: ShapeId = ShapeId::new("example#Operation", "example", "Operation");
        assert_eq!(ID.absolute(), "example#Operation");
        assert_eq!(ID.name(), "Operation");
        assert_eq!(ID.namespace(), "example");
    }

    #[test]
    fn schema_shape_id_converts_in_a_constant() {
        const SCHEMA_ID: aws_smithy_schema::ShapeId<'static> = aws_smithy_schema::shape_id!("example", "Operation");
        const ID: ShapeId = ShapeId::from_schema(&SCHEMA_ID);
        assert_eq!(ID, ShapeId::new("example#Operation", "example", "Operation"));
    }

    #[test]
    fn schema_shape_id_conversion_borrows_a_local_id() {
        let schema_id = aws_smithy_schema::shape_id!("example", "Operation");
        let id: ShapeId = (&schema_id).into();
        assert_eq!(id.absolute(), schema_id.as_str());
        assert_eq!(id.namespace(), schema_id.namespace());
        assert_eq!(id.name(), schema_id.shape_name());
        assert!(std::ptr::eq(id.absolute(), schema_id.as_str()));
        assert!(std::ptr::eq(id.namespace(), schema_id.namespace()));
        assert!(std::ptr::eq(id.name(), schema_id.shape_name()));
    }
}
