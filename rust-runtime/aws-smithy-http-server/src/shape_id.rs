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

/// Compatibility alias for a shared schema shape ID with static string components.
pub type ShapeId = aws_smithy_schema::ShapeId<'static>;

#[cfg(test)]
mod tests {
    use super::ShapeId;

    #[test]
    fn legacy_shape_id_api_uses_the_shared_schema_type() {
        const ID: ShapeId = ShapeId::new("example#Operation", "example", "Operation");
        let shared: aws_smithy_schema::ShapeId<'static> = ID;
        assert_eq!(shared.absolute(), shared.as_str());
        assert_eq!(shared.name(), shared.shape_name());
        assert_eq!(shared.namespace(), "example");
    }
}
