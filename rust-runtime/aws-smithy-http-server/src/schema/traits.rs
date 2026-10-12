/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed HTTP trait values stored in a schema's generic trait map.

use aws_smithy_schema::{Schema, ShapeId, Trait};
use std::any::Any;

/// The `smithy.api#httpError` trait on an error structure.
#[derive(Debug, Clone, Copy)]
pub struct HttpErrorTrait {
    code: u16,
}

impl HttpErrorTrait {
    /// The Shape ID for this trait.
    pub const TRAIT_ID: ShapeId<'static> = aws_smithy_schema::shape_id!("smithy.api", "httpError");

    /// Creates an HTTP error trait with the modeled status code.
    pub const fn new(code: u16) -> Self {
        Self { code }
    }

    /// Returns the modeled HTTP error status code.
    pub const fn code(&self) -> u16 {
        self.code
    }

    /// Reads this typed trait from a schema's fallback map.
    ///
    /// Initializes the lazy trait map on first access. Returns `None` if the
    /// schema has no map or the entry is absent or has a different Rust type.
    pub fn from_schema<'s>(schema: &'s Schema<'_>) -> Option<&'s Self> {
        schema.traits()?.get(&Self::TRAIT_ID)?.as_any().downcast_ref()
    }
}

impl Trait for HttpErrorTrait {
    fn trait_id(&self) -> &ShapeId<'static> {
        &Self::TRAIT_ID
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::{shape_id, ShapeType, StringTrait, TraitMap};
    use std::sync::LazyLock;

    static ERROR_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
        let mut map = TraitMap::new();
        map.insert(Box::new(StringTrait::new(shape_id!("smithy.api", "error"), "client")));
        map.insert(Box::new(HttpErrorTrait::new(404)));
        map
    });

    static ERROR: Schema<'static> =
        Schema::new_struct(shape_id!("example", "Missing"), ShapeType::Structure, &[]).with_traits(&ERROR_TRAITS);

    #[test]
    fn typed_http_error_survives_schema_cloning_and_preserves_trait_identity() {
        for schema in [&ERROR, &ERROR.clone()] {
            assert!(schema.http().is_none());
            let traits = schema.traits().unwrap();
            assert!(traits.contains_fqn("smithy.api#error"));
            assert!(traits.contains_fqn("smithy.api#httpError"));
            assert_eq!(traits.len(), 2);
            let http_error = HttpErrorTrait::from_schema(schema).unwrap();
            assert_eq!(http_error.code(), 404);
            assert_eq!(http_error.trait_id().as_str(), "smithy.api#httpError");
        }
    }

    #[test]
    fn absent_or_differently_typed_entries_return_none() {
        let plain = Schema::new(shape_id!("example", "Plain"), ShapeType::String);
        assert!(HttpErrorTrait::from_schema(&plain).is_none());
        let without_http_error: LazyLock<TraitMap> = LazyLock::new(|| {
            let mut map = TraitMap::new();
            map.insert(Box::new(StringTrait::new(shape_id!("smithy.api", "error"), "server")));
            map
        });
        assert!(HttpErrorTrait::from_schema(&plain.clone().with_traits(&without_http_error)).is_none());
        let differently_typed: LazyLock<TraitMap> = LazyLock::new(|| {
            let mut map = TraitMap::new();
            map.insert(Box::new(StringTrait::new(HttpErrorTrait::TRAIT_ID, "404")));
            map
        });
        assert!(HttpErrorTrait::from_schema(&plain.with_traits(&differently_typed)).is_none());
    }
}
