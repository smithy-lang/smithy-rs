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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeId {
    absolute: &'static str,

    namespace: &'static str,
    name: &'static str,
}

impl ShapeId {
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