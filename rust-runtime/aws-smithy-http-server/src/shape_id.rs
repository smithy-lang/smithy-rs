/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Extension types.
//!
//! Shape ID is a type that describes a Smithy shape.
//!

pub use crate::request::extension::{Extension, MissingExtension};

/// Shape ID for a modelled Smithy shape.
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

    /// Returns the Smithy model namespace.
    pub fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// Returns the Smithy operation name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the absolute operation shape ID.
    pub fn absolute(&self) -> &'static str {
        self.absolute
    }
}
