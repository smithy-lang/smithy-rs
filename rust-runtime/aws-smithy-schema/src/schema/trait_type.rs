/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::ShapeId;
use std::any::Any;
use std::fmt;

/// Trait representing a Smithy trait at runtime.
///
/// Traits provide additional metadata about shapes that affect serialization,
/// validation, and other behaviors.
pub trait Trait: Any + Send + Sync + fmt::Debug {
    /// Returns the Shape ID of this trait.
    fn trait_id(&self) -> &ShapeId;

    /// Returns this trait as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// An annotation trait (no value), e.g. `@sensitive`, `@sparse`, `@httpPayload`.
#[derive(Debug, Clone)]
pub struct AnnotationTrait {
    id: ShapeId,
}

impl AnnotationTrait {
    /// Creates a new annotation trait.
    pub fn new(id: ShapeId) -> Self {
        Self { id }
    }
}

impl Trait for AnnotationTrait {
    fn trait_id(&self) -> &ShapeId {
        &self.id
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A trait with a string value, e.g. `@jsonName("foo")`, `@xmlName("bar")`.
#[derive(Debug, Clone)]
pub struct StringTrait {
    id: ShapeId,
    value: String,
}

impl StringTrait {
    /// Creates a new string-valued trait.
    pub fn new(id: ShapeId, value: impl Into<String>) -> Self {
        Self {
            id,
            value: value.into(),
        }
    }

    /// Returns the string value of this trait.
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl Trait for StringTrait {
    fn trait_id(&self) -> &ShapeId {
        &self.id
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
