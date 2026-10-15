/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The Smithy prelude: shapes and traits defined in the `smithy.api` namespace.
//!
//! Models include the prelude by default (see
//! [`ModelAssembler::disable_prelude`](crate::ModelAssembler::disable_prelude)). The embedded
//! prelude is the one shipped with Smithy Java [`SOURCE_VERSION`].
//!
//! ```
//! use aws_smithy_lang::prelude;
//!
//! assert!(prelude::is_prelude_shape("smithy.api#String"));
//! assert!(!prelude::is_prelude_shape("com.example#String"));
//! assert_eq!(prelude::unit_id(), "smithy.api#Unit");
//! ```

use crate::json_loader::JsonLoader;
use crate::shape::Shape;
use crate::shape_id::ShapeId;
use crate::traits;
use indexmap::IndexMap;
use std::sync::OnceLock;

/// The prelude namespace, `smithy.api`.
pub const NAMESPACE: &str = "smithy.api";

/// The Smithy Java version whose prelude is embedded.
pub const SOURCE_VERSION: &str = "1.74.0";

/// `smithy.api#Unit`: the unit type, and the normalized target of an omitted operation input
/// or output.
pub const UNIT: ShapeId = ShapeId::from_static("smithy.api#Unit");

/// A `'static` reference to [`UNIT`].
pub fn unit_id() -> &'static ShapeId {
    static ID: ShapeId = UNIT;
    &ID
}

/// Returns `true` if `id` (a [`ShapeId`] or `&str`) is in the prelude namespace.
///
/// This checks only the namespace, like Java Smithy's `Prelude.isPreludeShape`. Use
/// [`is_public_prelude_shape`] to check that the prelude actually defines a shape.
pub fn is_prelude_shape(id: impl AsRef<str>) -> bool {
    id.as_ref()
        .split_once('#')
        .is_some_and(|(namespace, _)| namespace == NAMESPACE)
}

/// Returns `true` if the embedded prelude defines `id` (a root shape ID) and it is not marked
/// `@private`, so models outside the prelude may refer to it.
pub fn is_public_prelude_shape(id: impl AsRef<str>) -> bool {
    shapes()
        .get(id.as_ref())
        .is_some_and(|shape| !shape.traits().contains(traits::PRIVATE))
}

const PRELUDE_JSON: &str = include_str!("prelude/prelude-1.74.0.json");

/// The parsed prelude shapes, shared read-only by every model in the process.
pub(crate) fn shapes() -> &'static IndexMap<ShapeId, Shape> {
    static PRELUDE: OnceLock<IndexMap<ShapeId, Shape>> = OnceLock::new();
    PRELUDE.get_or_init(|| {
        let source = format!("smithy.api prelude ({SOURCE_VERSION})");
        JsonLoader::default()
            .parse_str(&source, PRELUDE_JSON)
            .expect("the embedded prelude is a valid Smithy 2.0 JSON AST document")
            .shapes
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert!(is_prelude_shape(UNIT));
        assert!(is_prelude_shape("smithy.api#Anything"));
        assert!(!is_prelude_shape("smithy.apiX#String"));
        assert!(!is_prelude_shape("not an id"));
        assert!(is_public_prelude_shape("smithy.api#String"));
        assert!(is_public_prelude_shape(traits::DOCUMENTATION));
        assert!(!is_public_prelude_shape("smithy.api#Missing"));
        let private = shapes()
            .values()
            .find(|s| s.traits().contains(traits::PRIVATE))
            .expect("the prelude has private shapes");
        assert!(is_prelude_shape(private.id()));
        assert!(!is_public_prelude_shape(private.id()));
        assert_eq!(unit_id(), &UNIT);
    }
}
